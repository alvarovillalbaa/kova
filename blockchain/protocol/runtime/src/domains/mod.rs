use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use blake3;
use bincode;

use crate::{Hash, FeeSplit};
use state::{DomainEntry, DomainType};

pub mod evm;
pub mod wasm;

pub use evm::EvmAdapter;
pub use wasm::WasmAdapter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainCall {
    pub domain_id: Uuid,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub raw: Vec<u8>,
    #[serde(default)]
    pub max_gas: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDomainMessage {
    pub from: Uuid,
    pub to: Uuid,
    pub nonce: u64,
    pub fee: u128,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainExecutionReceipt {
    pub domain_id: Uuid,
    pub state_root: Hash,
    pub gas_used: u64,
    pub events: Vec<String>,
    pub proof: Option<serde_json::Value>,
    pub trace: serde_json::Value,
    pub state: DomainState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudProof {
    pub domain_id: Uuid,
    pub claimed_root: Hash,
    pub witness: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DomainState {
    pub kv: HashMap<String, Vec<u8>>,
    pub inbox: Vec<CrossDomainMessage>,
    pub outbox: Vec<CrossDomainMessage>,
    pub next_out_nonce: u64,
    pub next_in_nonce: u64,
}

impl DomainState {
    pub fn root(&self) -> Hash {
        let mut leaves = Vec::new();
        for (k, v) in &self.kv {
            let mut data = k.as_bytes().to_vec();
            data.extend(v);
            leaves.push(*blake3::hash(&data).as_bytes());
        }
        for msg in &self.inbox {
            if let Ok(bytes) = bincode::serialize(msg) {
                leaves.push(*blake3::hash(&bytes).as_bytes());
            }
        }
        for msg in &self.outbox {
            if let Ok(bytes) = bincode::serialize(msg) {
                leaves.push(*blake3::hash(&bytes).as_bytes());
            }
        }
        leaves.push(*blake3::hash(&self.next_out_nonce.to_le_bytes()).as_bytes());
        leaves.push(*blake3::hash(&self.next_in_nonce.to_le_bytes()).as_bytes());
        if leaves.is_empty() {
            return [0u8; 32];
        }
        let mut hasher = blake3::Hasher::new();
        leaves.sort();
        for leaf in leaves {
            hasher.update(&leaf);
        }
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone)]
pub struct DomainStateStore {
    inner: Arc<Mutex<HashMap<Uuid, DomainState>>>,
}

impl DomainStateStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn load(&self, domain_id: &Uuid) -> DomainState {
        self.inner
            .lock()
            .unwrap()
            .get(domain_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn persist(&self, domain_id: &Uuid, state: DomainState) {
        self.inner.lock().unwrap().insert(*domain_id, state);
    }
}

pub struct DomainVmCtx<'a> {
    pub chain_id: &'a str,
    pub fee_split: &'a FeeSplit,
    pub block_height: u64,
    pub state: DomainState,
}

#[async_trait]
pub trait DomainVm: Send + Sync {
    fn kind(&self) -> DomainType;
    async fn execute(&self, call: &DomainCall, ctx: DomainVmCtx<'_>) -> anyhow::Result<DomainExecutionReceipt>;
}

enum DomainAdapter {
    Evm(Arc<EvmAdapter>),
    Wasm(Arc<WasmAdapter>),
}

impl DomainAdapter {
    fn kind(&self) -> DomainType {
        match self {
            DomainAdapter::Evm(a) => a.kind(),
            DomainAdapter::Wasm(a) => a.kind(),
        }
    }

    async fn execute(
        &self,
        call: &DomainCall,
        ctx: DomainVmCtx<'_>,
    ) -> anyhow::Result<DomainExecutionReceipt> {
        match self {
            DomainAdapter::Evm(vm) => vm.execute(call, ctx).await,
            DomainAdapter::Wasm(vm) => vm.execute(call, ctx).await,
        }
    }
}

#[derive(Clone)]
pub struct DomainRuntime {
    adapters: Arc<RwLock<HashMap<Uuid, DomainAdapter>>>,
    state: DomainStateStore,
    traces: Arc<RwLock<HashMap<Uuid, Vec<DomainExecutionReceipt>>>>,
}

impl Default for DomainRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainRuntime {
    pub fn new() -> Self {
        Self {
            adapters: Arc::new(RwLock::new(HashMap::new())),
            state: DomainStateStore::new(),
            traces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(&self, entry: &DomainEntry) -> anyhow::Result<()> {
        let adapter = match entry.kind {
            DomainType::EvmSharedSecurity => {
                DomainAdapter::Evm(Arc::new(EvmAdapter::new(entry.domain_id)))
            }
            DomainType::Wasm => DomainAdapter::Wasm(Arc::new(WasmAdapter::new(entry.domain_id))),
            _ => anyhow::bail!("unsupported domain kind {:?}", entry.kind),
        };
        self.adapters
            .write()
            .unwrap()
            .insert(entry.domain_id, adapter);
        Ok(())
    }

    pub fn has_domain(&self, id: &Uuid) -> bool {
        self.adapters.read().unwrap().contains_key(id)
    }

    pub fn next_out_nonce(&self, id: &Uuid) -> u64 {
        self.state.load(id).next_out_nonce
    }

    pub async fn execute(
        &self,
        call: &DomainCall,
        ctx: &crate::ExecutionContext<impl state::StateStore>,
        block_height: u64,
    ) -> anyhow::Result<DomainExecutionReceipt> {
        let adapters = self.adapters.read().unwrap();
        let adapter = adapters
            .get(&call.domain_id)
            .with_context(|| format!("domain {} not registered", call.domain_id))?;
        let domain_state = self.state.load(&call.domain_id);
        let vm_ctx = DomainVmCtx {
            chain_id: &ctx.chain_id,
            fee_split: &ctx.fee_split,
            block_height,
            state: domain_state.clone(),
        };
        drop(adapters);
        let mut receipt = adapter.execute(call, vm_ctx).await?;
        self.state.persist(&call.domain_id, receipt.state.clone());
        receipt.state_root = receipt.state.root();
        self.traces
            .write()
            .unwrap()
            .entry(call.domain_id)
            .or_default()
            .push(receipt.clone());
        Ok(receipt)
    }

    pub fn last_trace(&self, domain_id: &Uuid) -> Option<DomainExecutionReceipt> {
        self.traces
            .read()
            .unwrap()
            .get(domain_id)
            .and_then(|v| v.last().cloned())
    }

    pub fn latest_root(&self, domain_id: &Uuid) -> Option<Hash> {
        self.traces
            .read()
            .unwrap()
            .get(domain_id)
            .and_then(|v| v.last())
            .map(|r| r.state_root)
            .or_else(|| {
                let state = self.state.load(domain_id);
                Some(state.root())
            })
    }

    pub fn outbox(&self, domain_id: &Uuid) -> Vec<CrossDomainMessage> {
        self.state.load(domain_id).outbox
    }

    pub fn submit_fraud_proof(&self, proof: &FraudProof) -> anyhow::Result<()> {
        let traces = self.traces.read().unwrap();
        let Some(last) = traces.get(&proof.domain_id).and_then(|v| v.last()) else {
            anyhow::bail!("no execution trace for domain");
        };
        if last.state_root == proof.claimed_root {
            anyhow::bail!("claimed root already canonical");
        }
        if !serde_json::to_string(&proof.witness).is_ok() {
            anyhow::bail!("invalid witness");
        }
        Ok(())
    }

    pub fn push_outbox(&self, msg: CrossDomainMessage) {
        let mut state = self.state.load(&msg.from);
        state.outbox.push(msg);
        state.next_out_nonce = state.next_out_nonce.saturating_add(1);
        self.state.persist(&msg.from, state);
    }

    pub fn relay_message(&self, msg: CrossDomainMessage) -> anyhow::Result<()> {
        let mut dest = self.state.load(&msg.to);
        dest.inbox.push(msg.clone());
        dest.next_in_nonce = dest.next_in_nonce.saturating_add(1);
        self.state.persist(&msg.to, dest);
        Ok(())
    }
}
