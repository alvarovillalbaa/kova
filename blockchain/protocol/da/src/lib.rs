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
    pub commitment: DACommitment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DACommitment {
    pub root: Hash,
    pub total_shards: usize,
    pub data_shards: usize,
    pub parity_shards: usize,
    pub shard_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleProof {
    pub shard_index: usize,
    pub shard_hash: Hash,
    pub merkle_path: Vec<Hash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAProof {
    pub blob_id: String,
    pub commitment: DACommitment,
    pub samples: Vec<SampleProof>,
}

#[async_trait]
pub trait DAProvider: Send + Sync {
    async fn submit_blob(&self, domain_id: &str, blob_bytes: &[u8]) -> anyhow::Result<BlobRef>;
    async fn get_blob(&self, blob_id: &str) -> anyhow::Result<Vec<u8>>;
    async fn prove_blob_availability(&self, blob_id: &str) -> anyhow::Result<DAProof>;
    async fn get_commitment(&self, blob_id: &str) -> anyhow::Result<DACommitment>;
}

#[async_trait]
pub trait DASampler: Send + Sync {
    async fn sample(&self, blob_id: &str, samples: usize) -> anyhow::Result<bool>;
}

#[derive(Debug, Clone)]
pub struct DAConfig {
    pub shard_size: usize,
    pub data_shards: usize,
    pub parity_shards: usize,
}

impl Default for DAConfig {
    fn default() -> Self {
        Self {
            shard_size: 1024,
            data_shards: 4,
            parity_shards: 2,
        }
    }
}

#[derive(Clone, Default)]
pub struct InMemoryDA {
    inner: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    meta: Arc<Mutex<HashMap<String, BlobRef>>>,
    shards: Arc<Mutex<HashMap<String, Vec<Vec<u8>>>>>,
    commitments: Arc<Mutex<HashMap<String, DACommitment>>>,
    config: DAConfig,
}

impl InMemoryDA {
    pub fn new() -> Self {
        Self::with_config(DAConfig::default())
    }

    pub fn with_config(config: DAConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            meta: Arc::new(Mutex::new(HashMap::new())),
            shards: Arc::new(Mutex::new(HashMap::new())),
            commitments: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    fn shard_blob(&self, blob_bytes: &[u8]) -> (Vec<Vec<u8>>, DACommitment) {
        let cfg = &self.config;
        let mut data_shards = Vec::new();
        for chunk in blob_bytes.chunks(cfg.shard_size) {
            let mut shard = vec![0u8; cfg.shard_size];
            shard[..chunk.len()].copy_from_slice(chunk);
            data_shards.push(shard);
        }

        // pad data shards up to configured count
        while data_shards.len() < cfg.data_shards {
            data_shards.push(vec![0u8; cfg.shard_size]);
        }

        // parity shards = xor of all data shards
        let mut parity_shards = Vec::new();
        for _ in 0..cfg.parity_shards {
            let mut parity = vec![0u8; cfg.shard_size];
            for shard in &data_shards {
                for (i, byte) in shard.iter().enumerate() {
                    parity[i] ^= byte;
                }
            }
            parity_shards.push(parity);
        }

        let mut shards = data_shards;
        shards.extend(parity_shards);

        let leaf_hashes: Vec<Hash> = shards
            .iter()
            .map(|shard| *blake3::hash(shard).as_bytes())
            .collect();
        let root = merkle_root(&leaf_hashes);
        let commitment = DACommitment {
            root,
            total_shards: leaf_hashes.len(),
            data_shards: cfg.data_shards,
            parity_shards: cfg.parity_shards,
            shard_size: cfg.shard_size,
        };
        (shards, commitment)
    }
}

#[async_trait]
impl DAProvider for InMemoryDA {
    async fn submit_blob(&self, domain_id: &str, blob_bytes: &[u8]) -> anyhow::Result<BlobRef> {
        let id = format!("{}-{}", domain_id, uuid::Uuid::new_v4());
        let (shards, commitment) = self.shard_blob(blob_bytes);
        let mut guard = self.inner.lock().unwrap();
        guard.insert(id.clone(), blob_bytes.to_vec());
        self.shards.lock().unwrap().insert(id.clone(), shards);
        self.commitments
            .lock()
            .unwrap()
            .insert(id.clone(), commitment.clone());
        let blob_ref = BlobRef {
            id: id.clone(),
            domain_id: domain_id.to_string(),
            size_bytes: blob_bytes.len(),
            commitment: commitment.clone(),
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
        let shards_guard = self.shards.lock().unwrap();
        let Some(shards) = shards_guard.get(blob_id) else {
            anyhow::bail!("blob not found");
        };
        let commitment = self
            .commitments
            .lock()
            .unwrap()
            .get(blob_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("commitment missing"))?;
        let sample_count = (commitment.data_shards.max(1)).min(shards.len());
        let proofs = derive_sample_proofs(shards, &commitment, sample_count);
        Ok(DAProof {
            blob_id: blob_id.to_string(),
            commitment,
            samples: proofs,
        })
    }

    async fn get_commitment(&self, blob_id: &str) -> anyhow::Result<DACommitment> {
        self.commitments
            .lock()
            .unwrap()
            .get(blob_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("commitment missing"))
    }
}

#[async_trait]
impl DASampler for InMemoryDA {
    async fn sample(&self, blob_id: &str, samples: usize) -> anyhow::Result<bool> {
        let shards_guard = self.shards.lock().unwrap();
        let Some(shards) = shards_guard.get(blob_id) else {
            anyhow::bail!("blob not found");
        };
        let commitment = self
            .commitments
            .lock()
            .unwrap()
            .get(blob_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("commitment missing"))?;
        let proofs = derive_sample_proofs(shards, &commitment, samples.max(1));
        for sample in proofs {
            if !verify_merkle_path(sample.shard_hash, &sample.merkle_path, &commitment.root, sample.shard_index) {
                anyhow::bail!("invalid sampling proof");
            }
        }
        Ok(true)
    }
}

fn derive_sample_proofs(
    shards: &[Vec<u8>],
    commitment: &DACommitment,
    samples: usize,
) -> Vec<SampleProof> {
    let mut rng = StdRng::seed_from_u64(42);
    let mut proofs = Vec::new();
    if shards.is_empty() {
        return proofs;
    }
    for _ in 0..samples.min(shards.len()) {
        let idx = rng.gen_range(0..shards.len());
        let shard_hash = *blake3::hash(&shards[idx]).as_bytes();
        let merkle_path = merkle_proof(shards, idx);
        proofs.push(SampleProof {
            shard_index: idx,
            shard_hash,
            merkle_path,
        });
    }
    proofs
}

fn merkle_root(leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::new();
        for pair in level.chunks(2) {
            let combined = if pair.len() == 2 {
                [pair[0].as_slice(), pair[1].as_slice()].concat()
            } else {
                [pair[0].as_slice(), pair[0].as_slice()].concat()
            };
            next.push(*blake3::hash(&combined).as_bytes());
        }
        level = next;
    }
    level[0]
}

fn merkle_proof(shards: &[Vec<u8>], index: usize) -> Vec<Hash> {
    let leaves: Vec<Hash> = shards
        .iter()
        .map(|shard| *blake3::hash(shard).as_bytes())
        .collect();
    let mut idx = index;
    let mut level = leaves;
    let mut path = Vec::new();
    while level.len() > 1 {
        let sibling = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
        if sibling < level.len() {
            path.push(level[sibling]);
        } else {
            path.push(level[idx]); // duplicate if no sibling
        }
        let mut next = Vec::new();
        for pair in level.chunks(2) {
            let combined = if pair.len() == 2 {
                [pair[0].as_slice(), pair[1].as_slice()].concat()
            } else {
                [pair[0].as_slice(), pair[0].as_slice()].concat()
            };
            next.push(*blake3::hash(&combined).as_bytes());
        }
        idx /= 2;
        level = next;
    }
    path
}

fn verify_merkle_path(leaf: Hash, path: &[Hash], root: &Hash, mut index: usize) -> bool {
    let mut hash = leaf;
    for sibling in path {
        let combined = if index % 2 == 0 {
            [hash.as_slice(), sibling.as_slice()].concat()
        } else {
            [sibling.as_slice(), hash.as_slice()].concat()
        };
        hash = *blake3::hash(&combined).as_bytes();
        index /= 2;
    }
    &hash == root
}

pub fn verify_da_proof(proof: &DAProof) -> bool {
    for sample in &proof.samples {
        if !verify_merkle_path(
            sample.shard_hash,
            &sample.merkle_path,
            &proof.commitment.root,
            sample.shard_index,
        ) {
            return false;
        }
    }
    true
}

