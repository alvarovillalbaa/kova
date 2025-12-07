use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

fn hash_leaf(bytes: &[u8]) -> Hash {
    *blake3::hash(bytes).as_bytes()
}

fn fold_hashes(mut leaves: Vec<Hash>) -> Hash {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    leaves.sort();
    let mut hasher = blake3::Hasher::new();
    for leaf in leaves {
        hasher.update(&leaf);
    }
    *hasher.finalize().as_bytes()
}

pub type Address = [u8; 32];
pub type Hash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub address: Address,
    pub nonce: u64,
    pub balance_x: u128,
    pub code_hash: Option<Hash>,
    pub storage_root: Option<Hash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidatorStatus {
    Active,
    Jailed,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validator {
    pub owner: Address,
    pub id: Uuid,
    pub pubkey: Vec<u8>,
    pub stake: u128,
    pub status: ValidatorStatus,
    pub commission_rate: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    pub delegator: Address,
    pub validator_id: Uuid,
    pub stake: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainType {
    EvmSharedSecurity,
    Wasm,
    Privacy,
    Payment,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityModel {
    SharedSecurity,
    OwnSecurity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEntry {
    pub domain_id: Uuid,
    pub kind: DomainType,
    pub security_model: SecurityModel,
    pub sequencer_binding: Option<Uuid>,
    pub bridge_contracts: Vec<String>,
    pub risk_params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DACommitment {
    pub block_height: u64,
    pub da_root: Hash,
    pub blob_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainRoot {
    pub domain_id: Uuid,
    pub state_root: Hash,
    pub da_root: Hash,
    pub last_verified_epoch: u64,
    pub proof_meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: Uuid,
    pub payload: serde_json::Value,
    pub kind: String,
    pub status: String,
    pub votes: serde_json::Value,
    pub timers: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeePools {
    pub l1_gas: u128,
    pub da: u128,
    pub sequencer: u128,
    pub treasury: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyPool {
    pub merkle_root: Hash,
    pub parameters: serde_json::Value,
    pub nullifiers: Vec<Hash>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChainState {
    pub accounts: HashMap<Address, Account>,
    pub validators: HashMap<Uuid, Validator>,
    pub delegations: Vec<Delegation>,
    pub domains: HashMap<Uuid, DomainEntry>,
    pub da_commitments: Vec<DACommitment>,
    pub domain_roots: HashMap<Uuid, DomainRoot>,
    pub proposals: HashMap<Uuid, Proposal>,
    pub fee_pools: FeePools,
    pub privacy_pools: HashMap<String, PrivacyPool>,
}

impl ChainState {
    pub fn state_root(&self) -> Hash {
        let mut leaves = Vec::new();

        for account in self.accounts.values() {
            if let Ok(bytes) = bincode::serialize(account) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        for validator in self.validators.values() {
            if let Ok(bytes) = bincode::serialize(validator) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        for delegation in &self.delegations {
            if let Ok(bytes) = bincode::serialize(delegation) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        for domain in self.domains.values() {
            if let Ok(bytes) = bincode::serialize(domain) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        for da in &self.da_commitments {
            if let Ok(bytes) = bincode::serialize(da) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        for root in self.domain_roots.values() {
            if let Ok(bytes) = bincode::serialize(root) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        for proposal in self.proposals.values() {
            if let Ok(bytes) = bincode::serialize(proposal) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        if let Ok(bytes) = bincode::serialize(&self.fee_pools) {
            leaves.push(hash_leaf(&bytes));
        }

        for pool in self.privacy_pools.values() {
            if let Ok(bytes) = bincode::serialize(pool) {
                leaves.push(hash_leaf(&bytes));
            }
        }

        fold_hashes(leaves)
    }
}

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get_account(&self, address: &Address) -> anyhow::Result<Option<Account>>;
    async fn put_account(&self, account: Account) -> anyhow::Result<()>;
    async fn get_validator(&self, id: &Uuid) -> anyhow::Result<Option<Validator>>;
    async fn put_validator(&self, validator: Validator) -> anyhow::Result<()>;
    async fn get_chain_state(&self) -> anyhow::Result<ChainState>;
    async fn put_chain_state(&self, state: ChainState) -> anyhow::Result<()>;
    async fn commit(&self) -> anyhow::Result<Hash>;
}

#[derive(Clone, Default)]
pub struct InMemoryStateStore {
    inner: Arc<Mutex<ChainState>>,
}

impl InMemoryStateStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChainState::default())),
        }
    }
}

#[async_trait]
impl StateStore for InMemoryStateStore {
    async fn get_account(&self, address: &Address) -> anyhow::Result<Option<Account>> {
        let guard = self.inner.lock().unwrap();
        Ok(guard.accounts.get(address).cloned())
    }

    async fn put_account(&self, account: Account) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        guard.accounts.insert(account.address, account);
        Ok(())
    }

    async fn get_validator(&self, id: &Uuid) -> anyhow::Result<Option<Validator>> {
        let guard = self.inner.lock().unwrap();
        Ok(guard.validators.get(id).cloned())
    }

    async fn put_validator(&self, validator: Validator) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        guard.validators.insert(validator.id, validator);
        Ok(())
    }

    async fn get_chain_state(&self) -> anyhow::Result<ChainState> {
        let guard = self.inner.lock().unwrap();
        Ok(guard.clone())
    }

    async fn put_chain_state(&self, state: ChainState) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        *guard = state;
        Ok(())
    }

    async fn commit(&self) -> anyhow::Result<Hash> {
        let guard = self.inner.lock().unwrap();
        Ok(guard.state_root())
    }
}

#[derive(Default, Clone)]
pub struct SparseMerkleTree {
    leaves: HashMap<Vec<u8>, Hash>,
}

impl SparseMerkleTree {
    pub fn set(&mut self, key: &[u8], value: &[u8]) {
        self.leaves.insert(key.to_vec(), hash_leaf(value));
    }

    pub fn delete(&mut self, key: &[u8]) {
        self.leaves.remove(key);
    }

    pub fn root(&self) -> Hash {
        fold_hashes(self.leaves.values().cloned().collect())
    }
}

