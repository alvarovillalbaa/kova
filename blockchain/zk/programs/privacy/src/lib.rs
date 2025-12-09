use anyhow::{bail, Result};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use zk_core::{stub_proof, Commitments, ProgramId, ProofArtifact};

pub type Hash = [u8; 32];

/// Note commitment inputs for a shielded note.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub commitment: Hash,
    pub nullifier: Hash,
    pub amount: u128,
    pub recipient: Hash,
    pub merkle_root: Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyWithdrawInput {
    pub nullifier: Hash,
    pub merkle_root: Hash,
    pub recipient: Hash,
    pub amount: u128,
    pub commitment: Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyWithdrawOutput {
    pub nullifier: Hash,
    pub recipient: Hash,
    pub amount: u128,
    pub commitment: Hash,
}

/// Deterministically encode witness for the privacy withdraw circuit.
pub fn encode_input(input: &PrivacyWithdrawInput) -> Result<Vec<u8>> {
    Ok(bincode::serialize(input)?)
}

/// Commitments attached to the circuit; state root mirrors Merkle root to
/// anchor against the on-chain pool.
pub fn commitments(input: &PrivacyWithdrawInput) -> Commitments {
    Commitments {
        state_root: Some(input.merkle_root),
        da_root: None,
        events_root: Some(hash_bytes(&input.nullifier)),
        domain_root: Some(hash_bytes(&input.commitment)),
    }
}

/// Decode public outputs emitted by a real prover path. In stub mode these
/// bytes will be a deterministic hash; callers should gate on backend id when
/// using this helper.
pub fn decode_output(bytes: &[u8]) -> Result<PrivacyWithdrawOutput> {
    Ok(bincode::deserialize(bytes)?)
}

pub fn hash_bytes(data: &[u8]) -> Hash {
    let mut h = Hasher::new();
    h.update(data);
    *h.finalize().as_bytes()
}

/// Build a deterministic note commitment from components.
pub fn note_commitment(nullifier: &Hash, recipient: &Hash, amount: u128, salt: &[u8]) -> Hash {
    let mut h = Hasher::new();
    h.update(nullifier);
    h.update(recipient);
    h.update(&amount.to_le_bytes());
    h.update(salt);
    *h.finalize().as_bytes()
}

pub fn program_id() -> ProgramId {
    ProgramId::PrivacyWithdraw
}

/// Convenience to build a stub artifact for environments without a prover.
pub fn stub_withdraw_proof(input: &PrivacyWithdrawInput) -> Result<ProofArtifact> {
    let witness = encode_input(input)?;
    let commitments = commitments(input);
    Ok(stub_proof(program_id(), witness, Some(commitments)))
}

/// Verify a proof artifact locally when running in stub mode. Real provers
/// should be verified via the backend.
pub fn verify_stub_artifact(
    artifact: &ProofArtifact,
    input: &PrivacyWithdrawInput,
) -> Result<()> {
    let expected = stub_withdraw_proof(input)?;
    if artifact.backend != expected.backend {
        bail!("unexpected backend for stub verification");
    }
    if artifact.proof != expected.proof || artifact.public_outputs != expected.public_outputs {
        bail!("stub artifact mismatch");
    }
    if !commitments_match(&artifact.commitments, &expected.commitments) {
        bail!("stub commitments mismatch");
    }
    Ok(())
}

fn commitments_match(a: &Option<Commitments>, b: &Option<Commitments>) -> bool {
    match (a, b) {
        (Some(left), Some(right)) => {
            bincode::serialize(left).ok() == bincode::serialize(right).ok()
        }
        (None, None) => true,
        _ => false,
    }
}
