use async_trait::async_trait;
use runtime::{Hash, Tx};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct VmExecutionResult {
    pub state_root: Hash,
    pub gas_used: u64,
    pub events: Vec<String>,
}

#[async_trait]
pub trait VmHost: Send + Sync {
    async fn execute_tx(&self, tx: &Tx) -> anyhow::Result<VmExecutionResult>;
    async fn handle_block_begin(&self) -> anyhow::Result<()>;
    async fn handle_block_end(&self) -> anyhow::Result<()>;
}

#[async_trait]
pub trait Module: Send + Sync {
    async fn init(&self) -> anyhow::Result<()>;
    async fn handle_tx(&self, tx: &Tx) -> anyhow::Result<()>;
    async fn handle_block_begin(&self) -> anyhow::Result<()>;
    async fn handle_block_end(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct Precompile {
    pub id: String,
    pub description: String,
}

#[derive(Default, Clone)]
pub struct PrecompileRegistry {
    inner: HashMap<String, Precompile>,
}

impl PrecompileRegistry {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn register(&mut self, id: &str, description: &str) {
        self.inner.insert(
            id.to_string(),
            Precompile {
                id: id.to_string(),
                description: description.to_string(),
            },
        );
    }

    pub fn list(&self) -> Vec<Precompile> {
        self.inner.values().cloned().collect()
    }

    pub fn with_default_crypto() -> Self {
        let mut registry = Self::new();
        registry.register("poseidon", "Poseidon hash precompile");
        registry.register("keccak", "Keccak256 hash precompile");
        registry.register("sha2", "SHA2 hash precompile");
        registry.register("bls12-381", "BLS12-381 pairing helpers");
        registry.register("ed25519", "ED25519 signature verify");
        registry.register("secp256k1", "Secp256k1 signature verify");
        registry.register("zk-msm", "Multi-scalar multiplication accelerator");
        registry.register("zk-fft", "FFT helper for proofs");
        registry.register("merkle", "Merkle tree helper");
        registry.register("commitment", "Pedersen/commitment helper");
        registry
    }
}

