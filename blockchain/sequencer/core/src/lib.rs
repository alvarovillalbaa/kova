use async_trait::async_trait;
use da::{BlobRef, DAProvider, InMemoryDA};
use runtime::Tx;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedBatch {
    pub domain_id: String,
    pub batch_id: String,
    pub txs: Vec<Tx>,
    pub da_blob: Option<BlobRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStatus {
    pub batch_id: String,
    pub posted: bool,
    pub blob_ref: Option<BlobRef>,
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

