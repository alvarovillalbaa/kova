use async_trait::async_trait;
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
/// Alias to keep hashes consistent with the runtime layer.
pub type Hash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProgramId {
    Block,
    Rollup,
    PrivacyWithdraw,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commitments {
    pub state_root: Option<Hash>,
    pub da_root: Option<Hash>,
    pub events_root: Option<Hash>,
    pub domain_root: Option<Hash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofArtifact {
    pub backend: String,
    pub program_id: ProgramId,
    pub proof: Vec<u8>,
    pub public_outputs: Vec<u8>,
    pub commitments: Option<Commitments>,
    pub verification_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofRequest {
    pub program_id: ProgramId,
    pub witness: Vec<u8>,
    pub commitments: Option<Commitments>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramDescriptor {
    pub id: ProgramId,
    pub name: String,
    pub description: String,
    pub version: String,
}

#[derive(Default, Clone)]
pub struct ProgramRegistry {
    programs: HashMap<ProgramId, ProgramDescriptor>,
}

impl ProgramRegistry {
    pub fn new() -> Self {
        Self {
            programs: HashMap::new(),
        }
    }

    pub fn register(&mut self, descriptor: ProgramDescriptor) {
        self.programs.insert(descriptor.id.clone(), descriptor);
    }

    pub fn get(&self, id: &ProgramId) -> Option<&ProgramDescriptor> {
        self.programs.get(id)
    }

    pub fn list(&self) -> Vec<ProgramDescriptor> {
        self.programs.values().cloned().collect()
    }
}

#[derive(Debug, Error)]
pub enum ZkError {
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("program not registered: {0:?}")]
    UnknownProgram(ProgramId),
    #[error("proof rejected: {0}")]
    ProofRejected(String),
    #[error("invalid commitment: {0}")]
    InvalidCommitment(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("other: {0}")]
    Other(String),
}

pub type ZkResult<T> = Result<T, ZkError>;

#[async_trait]
pub trait ZkBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn registry(&self) -> &ProgramRegistry;
    async fn prove(&self, request: ProofRequest) -> ZkResult<ProofArtifact>;
    async fn verify(&self, artifact: &ProofArtifact) -> ZkResult<()>;
}

#[derive(Default, Clone)]
pub struct BackendRegistry {
    backends: HashMap<String, Arc<dyn ZkBackend>>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
        }
    }

    pub fn register(&mut self, backend: Arc<dyn ZkBackend>) {
        self.backends
            .insert(backend.backend_id().to_string(), backend);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ZkBackend>> {
        self.backends.get(id).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.backends.keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockProof {
    pub block_hash: Hash,
    pub state_root: Hash,
    pub proof: ProofArtifact,
}

/// Deterministic commitment helper for stubbed backends.
pub fn blake3_commit(data: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

/// Helper to build a deterministic proof artifact when using stub mode.
pub fn stub_proof(program_id: ProgramId, witness: Vec<u8>, commitments: Option<Commitments>) -> ProofArtifact {
    let mut buf = witness.clone();
    if let Some(c) = &commitments {
        if let Some(sr) = c.state_root {
            buf.extend_from_slice(&sr);
        }
        if let Some(dr) = c.da_root {
            buf.extend_from_slice(&dr);
        }
        if let Some(er) = c.events_root {
            buf.extend_from_slice(&er);
        }
        if let Some(dom) = c.domain_root {
            buf.extend_from_slice(&dom);
        }
    }
    let commitment = blake3_commit(&buf);
    ProofArtifact {
        backend: "stub".into(),
        program_id,
        proof: commitment.to_vec(),
        public_outputs: commitment.to_vec(),
        commitments,
        verification_key: None,
    }
}
