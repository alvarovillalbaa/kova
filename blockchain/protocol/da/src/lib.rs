use async_trait::async_trait;
use blake3;
use rand::{rngs::StdRng, Rng, SeedableRng};
use runtime::Hash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRef {
    pub id: String,
    pub domain_id: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAProof {
    pub blob_id: String,
    pub samples: Vec<Hash>,
}

#[async_trait]
pub trait DAProvider: Send + Sync {
    async fn submit_blob(&self, domain_id: &str, blob_bytes: &[u8]) -> anyhow::Result<BlobRef>;
    async fn get_blob(&self, blob_id: &str) -> anyhow::Result<Vec<u8>>;
    async fn prove_blob_availability(&self, blob_id: &str) -> anyhow::Result<DAProof>;
}

#[async_trait]
pub trait DASampler: Send + Sync {
    async fn sample(&self, blob_id: &str, samples: usize) -> anyhow::Result<bool>;
}

#[derive(Clone, Default)]
pub struct InMemoryDA {
    inner: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    meta: Arc<Mutex<HashMap<String, BlobRef>>>,
}

impl InMemoryDA {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            meta: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl DAProvider for InMemoryDA {
    async fn submit_blob(&self, domain_id: &str, blob_bytes: &[u8]) -> anyhow::Result<BlobRef> {
        let id = format!("{}-{}", domain_id, uuid::Uuid::new_v4());
        let mut guard = self.inner.lock().unwrap();
        guard.insert(id.clone(), blob_bytes.to_vec());
        let blob_ref = BlobRef {
            id: id.clone(),
            domain_id: domain_id.to_string(),
            size_bytes: blob_bytes.len(),
        };
        self.meta.lock().unwrap().insert(id.clone(), blob_ref.clone());
        Ok(blob_ref)
    }

    async fn get_blob(&self, blob_id: &str) -> anyhow::Result<Vec<u8>> {
        let guard = self.inner.lock().unwrap();
        guard
            .get(blob_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("blob not found"))
    }

    async fn prove_blob_availability(&self, blob_id: &str) -> anyhow::Result<DAProof> {
        let guard = self.inner.lock().unwrap();
        if let Some(blob) = guard.get(blob_id) {
            let samples = derive_samples(blob, 8);
            Ok(DAProof {
                blob_id: blob_id.to_string(),
                samples,
            })
        } else {
            anyhow::bail!("blob not found")
        }
    }
}

#[async_trait]
impl DASampler for InMemoryDA {
    async fn sample(&self, blob_id: &str, samples: usize) -> anyhow::Result<bool> {
        let guard = self.inner.lock().unwrap();
        if guard.contains_key(blob_id) {
            let blob = guard.get(blob_id).unwrap();
            let required = samples.max(1);
            let proof_samples = derive_samples(blob, required);
            Ok(proof_samples.len() >= required)
        } else {
            anyhow::bail!("blob not found")
        }
    }
}

fn derive_samples(blob: &[u8], samples: usize) -> Vec<Hash> {
    let mut rng = StdRng::seed_from_u64(42);
    let mut out = Vec::new();
    if blob.is_empty() {
        return out;
    }
    for _ in 0..samples {
        let idx = rng.gen_range(0..blob.len());
        let chunk = &blob[idx..std::cmp::min(blob.len(), idx + 32)];
        out.push(*blake3::hash(chunk).as_bytes());
    }
    out
}

