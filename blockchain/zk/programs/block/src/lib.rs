use anyhow::Result;
use blake3::Hasher;
use runtime::{Block, Hash};
use serde::{Deserialize, Serialize};
use zk_core::{Commitments, ProgramId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockProgramWitness {
    pub block: Block,
    pub post_state_root: Hash,
    pub events_root: Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockProgramOutput {
    pub state_root: Hash,
    pub events_root: Hash,
    pub gas_used: u64,
}

pub fn encode_witness(block: &Block, post_state_root: Hash, events: &[String], gas_used: u64) -> Result<Vec<u8>> {
    let events_root = hash_events(events);
    let witness = BlockProgramWitness {
        block: block.clone(),
        post_state_root,
        events_root,
    };
    let mut bytes = bincode::serialize(&witness)?;
    bytes.extend_from_slice(&gas_used.to_le_bytes());
    Ok(bytes)
}

pub fn commitments(post_state_root: Hash, events_root: Hash, da_root: Hash) -> Commitments {
    Commitments {
        state_root: Some(post_state_root),
        da_root: Some(da_root),
        events_root: Some(events_root),
        domain_root: None,
    }
}

pub fn decode_output(bytes: &[u8]) -> Result<BlockProgramOutput> {
    if bytes.len() < std::mem::size_of::<u64>() {
        anyhow::bail!("output too small");
    }
    let (witness_bytes, gas_bytes) = bytes.split_at(bytes.len() - std::mem::size_of::<u64>());
    let witness: BlockProgramWitness = bincode::deserialize(witness_bytes)?;
    let mut gas_arr = [0u8; 8];
    gas_arr.copy_from_slice(gas_bytes);
    Ok(BlockProgramOutput {
        state_root: witness.post_state_root,
        events_root: witness.events_root,
        gas_used: u64::from_le_bytes(gas_arr),
    })
}

pub fn hash_events(events: &[String]) -> Hash {
    let mut hasher = Hasher::new();
    for e in events {
        hasher.update(e.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

pub fn program_id() -> ProgramId {
    ProgramId::Block
}
