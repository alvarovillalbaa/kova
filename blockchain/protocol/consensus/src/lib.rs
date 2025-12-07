use async_trait::async_trait;
use blake3;
use runtime::{hash_block, Block, BlockHeader, Hash, Tx};
use serde::{Deserialize, Serialize};
use state::Validator;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::time::{self, Duration};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashEvidence {
    pub validator_id: Uuid,
    pub reason: String,
    pub height: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConsensusMetrics {
    pub current_view: u64,
    pub locked_qc: bool,
    pub pending_qc: bool,
    pub commit_queue_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumCertificate {
    pub block_id: Hash,
    pub view: u64,
    pub signatures: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusState {
    pub view: u64,
    pub height: u64,
    pub locked_qc: Option<QuorumCertificate>,
    pub pending_qc: Option<QuorumCertificate>,
}

#[async_trait]
pub trait ConsensusEngine: Send + Sync {
    async fn propose(&self, block: Block) -> anyhow::Result<()>;
    async fn vote(&self, block_id: Hash, view: u64, voter: &Validator) -> anyhow::Result<()>;
    async fn on_qc(&self, qc: QuorumCertificate) -> anyhow::Result<()>;
    async fn on_timeout(&self, view: u64) -> anyhow::Result<()>;
    async fn validator_set(&self) -> anyhow::Result<Vec<Validator>>;
    async fn record_slash(&self, evidence: SlashEvidence) -> anyhow::Result<()>;
    fn metrics(&self) -> ConsensusMetrics;
    fn pop_commit(&self) -> Option<Hash>;
    fn leader_for_view(&self, view: u64) -> Option<Validator>;
    fn current_view(&self) -> u64;
}

pub fn build_block(header: BlockHeader, txs: Vec<Tx>, da_blobs: Vec<String>) -> Block {
    Block {
        header,
        transactions: txs,
        da_blobs,
    }
}

#[derive(Clone)]
pub struct HotStuffEngine {
    inner: Arc<Mutex<HotStuffInner>>,
    timeout: Duration,
}

#[derive(Debug)]
struct HotStuffInner {
    state: ConsensusState,
    pending_blocks: HashMap<Hash, Block>,
    block_tree: HashMap<Hash, Block>,
    votes: HashMap<(Hash, u64), VoteTally>,
    validators: Vec<Validator>,
    commit_queue: VecDeque<Hash>,
    total_stake: u128,
}

#[derive(Debug, Default, Clone)]
struct VoteTally {
    stake: u128,
    voters: Vec<Uuid>,
}

impl HotStuffEngine {
    pub fn new(validators: Vec<Validator>) -> Self {
        let inner = HotStuffInner {
            state: ConsensusState {
                view: 0,
                height: 0,
                locked_qc: None,
                pending_qc: None,
            },
            pending_blocks: HashMap::new(),
            block_tree: HashMap::new(),
            votes: HashMap::new(),
            total_stake: validators.iter().map(|v| v.stake).sum(),
            validators,
            commit_queue: VecDeque::new(),
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
            timeout: Duration::from_millis(1_500),
        }
    }

    pub async fn run_timeouts(self) {
        let mut interval = time::interval(self.timeout);
        loop {
            interval.tick().await;
            let view = { self.inner.lock().unwrap().state.view };
            let _ = self.on_timeout(view).await;
        }
    }
}

impl HotStuffInner {
    fn quorum_threshold(&self) -> u128 {
        (self.total_stake * 2) / 3 + 1
    }

    fn leader_for_view(&self, view: u64) -> Option<Validator> {
        if self.validators.is_empty() {
            return None;
        }
        let mut slot = (view as u128) % self.total_stake.max(1);
        for v in &self.validators {
            if slot < v.stake {
                return Some(v.clone());
            }
            slot = slot.saturating_sub(v.stake);
        }
        self.validators.first().cloned()
    }
}

#[async_trait]
impl ConsensusEngine for HotStuffEngine {
    async fn propose(&self, block: Block) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(leader) = guard.leader_for_view(guard.state.view) {
            if leader.owner != block.header.proposer_id {
                tracing::warn!("proposal from non-leader for view {}", guard.state.view);
            }
        }
        let block_id = hash_block(&block);
        guard.pending_blocks.insert(block_id, block);
        guard.block_tree.insert(block_id, guard.pending_blocks[&block_id].clone());
        guard.state.view += 1;
        Ok(())
    }

    async fn vote(&self, block_id: Hash, view: u64, voter: &Validator) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        let tally = guard
            .votes
            .entry((block_id, view))
            .or_insert_with(VoteTally::default);

        if tally.voters.contains(&voter.id) {
            return Ok(()); // ignore duplicate vote
        }

        tally.voters.push(voter.id);
        tally.stake = tally.stake.saturating_add(voter.stake);

        if tally.stake >= guard.quorum_threshold() {
            let qc = QuorumCertificate {
                block_id,
                view,
                signatures: vec![],
            };
            guard.state.pending_qc = Some(qc.clone());
            guard.state.locked_qc = Some(qc.clone());
            guard.commit_queue.push_back(block_id);
        }
        Ok(())
    }

    async fn on_qc(&self, qc: QuorumCertificate) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        guard.state.locked_qc = Some(qc.clone());
        guard.state.height += 1;

        // 3-chain commit simulation: commit parent of qc.block_id if exists.
        if let Some(current) = guard.block_tree.get(&qc.block_id) {
            let parent_hash = current.header.parent_hash;
            if parent_hash != [0u8; 32] {
                guard.commit_queue.push_back(parent_hash);
            }
        }

        Ok(())
    }

    async fn on_timeout(&self, view: u64) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        if view == guard.state.view {
            guard.state.view += 1;
        }
        Ok(())
    }

    async fn validator_set(&self) -> anyhow::Result<Vec<Validator>> {
        let guard = self.inner.lock().unwrap();
        Ok(guard.validators.clone())
    }

    async fn record_slash(&self, evidence: SlashEvidence) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        // Placeholder: slashing is enforced by runtime; consensus records evidence for observability.
        let digest = blake3::hash(evidence.validator_id.as_bytes());
        guard.commit_queue.push_back(*digest.as_bytes());
        Ok(())
    }

    fn metrics(&self) -> ConsensusMetrics {
        let guard = self.inner.lock().unwrap();
        ConsensusMetrics {
            current_view: guard.state.view,
            locked_qc: guard.state.locked_qc.is_some(),
            pending_qc: guard.state.pending_qc.is_some(),
            commit_queue_depth: guard.commit_queue.len(),
        }
    }

    fn pop_commit(&self) -> Option<Hash> {
        let mut guard = self.inner.lock().unwrap();
        guard.commit_queue.pop_front()
    }

    fn leader_for_view(&self, view: u64) -> Option<Validator> {
        let guard = self.inner.lock().unwrap();
        guard.leader_for_view(view)
    }

    fn current_view(&self) -> u64 {
        let guard = self.inner.lock().unwrap();
        guard.state.view
    }
}

