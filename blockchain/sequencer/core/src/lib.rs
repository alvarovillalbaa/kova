use async_trait::async_trait;
use da::{BlobRef, DAProvider, InMemoryDA};
use runtime::Tx;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};
use uuid::Uuid;
use blake3;
use zk_core::{ProgramId, ProofArtifact, ProofRequest, ZkBackend};
use zk_program_rollup::{commitments as rollup_commitments, encode_input as encode_rollup_input, RollupProofInput};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedBatch {
    pub domain_id: String,
    pub batch_id: String,
    pub txs: Vec<Tx>,
    pub da_blob: Option<BlobRef>,
    pub proof: Option<ProofArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStatus {
    pub batch_id: String,
    pub posted: bool,
    pub blob_ref: Option<BlobRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencerInfo {
    pub id: String,
    pub stake: u128,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RotationPolicy {
    RoundRobin,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        RotationPolicy::RoundRobin
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashEvent {
    pub sequencer_id: String,
    pub reason: String,
}

#[derive(Clone, Default)]
pub struct SequencerSet {
    members: Arc<Mutex<Vec<SequencerInfo>>>,
    policy: RotationPolicy,
    force_include: Arc<Mutex<VecDeque<String>>>,
    pub slashing_events: Arc<Mutex<Vec<SlashEvent>>>,
}

impl SequencerSet {
    pub fn new(members: Vec<SequencerInfo>, policy: RotationPolicy) -> Self {
        Self {
            members: Arc::new(Mutex::new(members)),
            policy,
            force_include: Arc::new(Mutex::new(VecDeque::new())),
            slashing_events: Arc::new(Mutex::new(vec![])),
        }
    }

    pub fn active_leader(&self, round: u64) -> Option<SequencerInfo> {
        let members = self.members.lock().unwrap();
        if members.is_empty() {
            return None;
        }
        match self.policy {
            RotationPolicy::RoundRobin => members.get((round as usize) % members.len()).cloned(),
        }
    }

    pub fn member_count(&self) -> usize {
        self.members.lock().unwrap().len()
    }

    pub fn enqueue_force_include(&self, blob_id: String) {
        self.force_include.lock().unwrap().push_back(blob_id);
    }

    pub fn pop_force_include(&self) -> Option<String> {
        self.force_include.lock().unwrap().pop_front()
    }

    pub fn slash(&self, sequencer_id: &str, reason: &str) -> SlashEvent {
        let event = SlashEvent {
            sequencer_id: sequencer_id.to_string(),
            reason: reason.to_string(),
        };
        self.slashing_events.lock().unwrap().push(event.clone());
        event
    }
}

#[async_trait]
pub trait Sequencer: Send + Sync {
    async fn submit_tx(&self, domain_id: &str, tx: Tx) -> anyhow::Result<()>;
    async fn build_batch(&self, domain_id: &str) -> anyhow::Result<SequencedBatch>;
    async fn domain_head(&self, domain_id: &str) -> anyhow::Result<u64>;
    async fn batch_status(&self, domain_id: &str, batch_id: &str) -> anyhow::Result<Option<BatchStatus>>;
}

pub struct InMemorySequencer {
    pub pending: Arc<Mutex<Vec<(String, Tx)>>>,
    pub da: InMemoryDA,
    pub batches: Arc<Mutex<HashMap<String, Vec<SequencedBatch>>>>,
    pub heads: Arc<Mutex<HashMap<String, u64>>>,
    pub zk: Option<Arc<dyn ZkBackend>>,
}

#[async_trait]
impl Sequencer for InMemorySequencer {
    async fn submit_tx(&self, domain_id: &str, tx: Tx) -> anyhow::Result<()> {
        info!("queued tx for domain {}", domain_id);
        self.pending
            .lock()
            .unwrap()
            .push((domain_id.to_string(), tx));
        Ok(())
    }

    async fn build_batch(&self, domain_id: &str) -> anyhow::Result<SequencedBatch> {
        let mut pending = self.pending.lock().unwrap();
        let mut txs = Vec::new();
        let mut remaining = Vec::new();
        for (d, tx) in pending.drain(..) {
            if d == domain_id {
                txs.push(tx);
            } else {
                remaining.push((d, tx));
            }
        }
        *pending = remaining;
        let blob = if !txs.is_empty() {
            let bytes = serde_json::to_vec(&txs)?;
            Some(self.da.submit_blob(domain_id, &bytes).await?)
        } else {
            None
        };
        let proof = if let (Some(zk), Some(blob_ref)) = (self.zk.clone(), blob.clone()) {
            match Uuid::parse_str(domain_id) {
                Ok(domain_uuid) => {
                    let da_root = blob_ref.commitment.root;
                    let input = RollupProofInput {
                        domain_id: domain_uuid,
                        blob_id: blob_ref.id.clone(),
                        da_root,
                        state_root: [0u8; 32],
                        batch_bytes: serde_json::to_vec(&txs)?,
                    };
                    let witness = encode_rollup_input(&input)?;
                    let commitments = rollup_commitments(&input);
                    match zk
                        .prove(ProofRequest {
                            program_id: ProgramId::Rollup,
                            witness,
                            commitments: Some(commitments),
                        })
                        .await
                    {
                        Ok(artifact) => {
                            if let Err(err) = zk.verify(&artifact).await {
                                warn!("rollup proof verification failed: {err}");
                                None
                            } else {
                                Some(artifact)
                            }
                        }
                        Err(err) => {
                            warn!("rollup proof generation failed: {err}");
                            None
                        }
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        };
        let mut batches = self.batches.lock().unwrap();
        let next_id = batches
            .get(domain_id)
            .map(|v| v.len() as u64)
            .unwrap_or(0);
        let batch = SequencedBatch {
            domain_id: domain_id.to_string(),
            batch_id: format!("{}-{}", domain_id, next_id),
            txs,
            da_blob: blob.clone(),
            proof,
        };
        batches.entry(domain_id.to_string()).or_default().push(batch.clone());
        let mut heads = self.heads.lock().unwrap();
        let height = heads.entry(domain_id.to_string()).or_insert(0);
        *height += 1;
        Ok(batch)
    }

    async fn domain_head(&self, domain_id: &str) -> anyhow::Result<u64> {
        let heads = self.heads.lock().unwrap();
        Ok(*heads.get(domain_id).unwrap_or(&0))
    }

    async fn batch_status(&self, domain_id: &str, batch_id: &str) -> anyhow::Result<Option<BatchStatus>> {
        let batches = self.batches.lock().unwrap();
        let Some(list) = batches.get(domain_id) else {
            return Ok(None);
        };
        let status = list
            .iter()
            .find(|b| b.batch_id == batch_id)
            .map(|b| BatchStatus {
                batch_id: b.batch_id.clone(),
                posted: b.da_blob.is_some(),
                blob_ref: b.da_blob.clone(),
            });
        Ok(status)
    }
}

