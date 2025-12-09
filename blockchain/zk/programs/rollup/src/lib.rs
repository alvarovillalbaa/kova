use anyhow::Result;
use blake3::Hasher;
use runtime::Hash;
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use zk_core::{Commitments, ProgramId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupProofInput {
    pub domain_id: Uuid,
    pub blob_id: String,
    pub da_root: Hash,
    pub state_root: Hash,
    pub batch_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupProofOutput {
    pub domain_id: Uuid,
    pub da_root: Hash,
    pub state_root: Hash,
    pub batch_commitment: Hash,
}

pub fn encode_input(input: &RollupProofInput) -> Result<Vec<u8>> {
    Ok(bincode::serialize(input)?)
}

pub fn commitments(input: &RollupProofInput) -> Commitments {
    Commitments {
        state_root: Some(input.state_root),
        da_root: Some(input.da_root),
        events_root: Some(hash_blob(&input.batch_bytes)),
        domain_root: Some(hash_blob(&input.domain_id.as_bytes())),
    }
}

pub fn decode_output(bytes: &[u8]) -> Result<RollupProofOutput> {
    Ok(bincode::deserialize(bytes)?)
}

pub fn hash_blob(data: &[u8]) -> Hash {
    let mut h = Hasher::new();
    h.update(data);
    *h.finalize().as_bytes()
}

pub fn program_id() -> ProgramId {
    ProgramId::Rollup
}
