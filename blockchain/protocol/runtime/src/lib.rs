use serde::{Deserialize, Serialize};
use state::{
    Account, ChainState, Delegation, FeePools, InMemoryStateStore, StateStore, Validator,
    ValidatorStatus,
};
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub type Address = [u8; 32];
pub type Hash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TxPayload {
    Transfer { to: Address, amount: u128 },
    Stake { amount: u128 },
    Unstake { amount: u128 },
    Delegate { validator: Address, amount: u128 },
    Undelegate { validator: Address, amount: u128 },
    DomainCreate { domain_id: Uuid, params: serde_json::Value },
    DomainConfigUpdate { domain_id: Uuid, params: serde_json::Value },
    RollupBatchCommit { domain_id: Uuid, blob_id: String },
    RollupBridgeDeposit { domain_id: Uuid, amount: u128 },
    RollupBridgeWithdraw { domain_id: Uuid, amount: u128 },
    GovernanceProposal { payload: serde_json::Value },
    GovernanceVote { proposal_id: Uuid, support: bool, weight: u128 },
    PrivacyDeposit { commitment: Hash },
    PrivacyWithdraw { nullifier: Hash, recipient: Address },
    SystemUpgrade { module: String, version: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tx {
    pub chain_id: String,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee: Option<u128>,
    pub max_priority_fee: Option<u128>,
    pub gas_price: Option<u128>,
    pub payload: TxPayload,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub parent_hash: Hash,
    pub height: u64,
    pub timestamp: u64,
    pub proposer_id: Address,
    pub state_root: Hash,
    pub l1_tx_root: Hash,
    pub da_root: Hash,
    pub domain_roots: Vec<Hash>,
    pub gas_used: u64,
    pub gas_limit: u64,
    pub base_fee: u128,
    pub consensus_metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Tx>,
    pub da_blobs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeSplit {
    pub l1_gas_burn_pct: u8,
    pub l1_gas_validators_pct: u8,
    pub da_validators_pct: u8,
    pub da_nodes_pct: u8,
    pub da_treasury_pct: u8,
    pub l2_sequencer_pct: u8,
    pub l2_da_costs_pct: u8,
    pub l2_l1_rent_pct: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    pub pubkey: Vec<u8>,
    pub stake: u128,
    pub commission_rate: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub chain_id: String,
    pub initial_validators: Vec<GenesisValidator>,
    pub initial_accounts: Vec<(Address, u128)>,
    pub block_time_ms: u64,
    pub max_gas_per_block: u64,
    pub base_fee: u128,
    pub da_sample_count: u16,
    pub slashing_double_sign: u8,
    pub fee_split: FeeSplit,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext<S: StateStore> {
    pub state: S,
    pub fee_split: FeeSplit,
    pub chain_id: String,
    pub base_fee: u128,
    pub max_gas_per_block: u64,
    pub block_time_ms: u64,
    pub da_sample_count: u16,
    pub slashing_double_sign: u8,
}

impl<S: StateStore> ExecutionContext<S> {
    pub fn new(
        state: S,
        fee_split: FeeSplit,
        chain_id: String,
        base_fee: u128,
        max_gas_per_block: u64,
        block_time_ms: u64,
        da_sample_count: u16,
        slashing_double_sign: u8,
    ) -> Self {
        Self {
            state,
            fee_split,
            chain_id,
            base_fee,
            max_gas_per_block,
            block_time_ms,
            da_sample_count,
            slashing_double_sign,
        }
    }
}

pub async fn apply_tx<S: StateStore>(
    ctx: &ExecutionContext<S>,
    tx: &Tx,
) -> anyhow::Result<ExecutionOutcome> {
    if tx.chain_id != ctx.chain_id {
        anyhow::bail!("invalid chain id");
    }

    let sender = derive_sender(&tx.signature);
    let mut sender_account = ctx
        .state
        .get_account(&sender)
        .await?
        .unwrap_or(default_account(sender));

    if sender_account.nonce != tx.nonce {
        anyhow::bail!("invalid nonce");
    }

    let gas_used = gas_cost(&tx.payload);
    let gas_price = effective_gas_price(tx, ctx.base_fee)?;
    let gas_fee = (gas_used as u128)
        .checked_mul(gas_price)
        .ok_or_else(|| anyhow::anyhow!("gas fee overflow"))?;

    let mut chain = ctx.state.get_chain_state().await?;

    match &tx.payload {
        TxPayload::Transfer { to, amount } => {
            ensure_funds(&sender_account, *amount, gas_fee)?;
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(*amount + gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;

            let mut to_account = ctx.state.get_account(to).await?.unwrap_or(default_account(*to));
            to_account.balance_x = to_account
                .balance_x
                .checked_add(*amount)
                .ok_or_else(|| anyhow::anyhow!("overflow"))?;
            ctx.state.put_account(to_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;

            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["transfer".into()],
            ))
        }
        TxPayload::Stake { amount } => {
            ensure_funds(&sender_account, *amount, gas_fee)?;
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(*amount + gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            // ensure chain state fetched early stays accurate
            if let Some(v) = chain.validators.values_mut().find(|v| v.owner == sender) {
                v.stake = v
                    .stake
                    .checked_add(*amount)
                    .ok_or_else(|| anyhow::anyhow!("stake overflow"))?;
                v.status = ValidatorStatus::Active;
            } else {
                let id = Uuid::new_v4();
                let validator = Validator {
                    owner: sender,
                    id,
                    pubkey: tx.signature.clone(),
                    stake: *amount,
                    status: ValidatorStatus::Active,
                    commission_rate: 0,
                };
                chain.validators.insert(id, validator);
            }
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(gas_used, vec!["stake".into()]))
        }
        TxPayload::Unstake { amount } => {
            let Some(v) = chain.validators.values_mut().find(|v| v.owner == sender) else {
                anyhow::bail!("no validator for sender");
            };
            if v.stake < *amount {
                anyhow::bail!("insufficient staked amount");
            }
            v.stake -= *amount;
            if v.stake == 0 {
                v.status = ValidatorStatus::Exited;
            }
            ctx.state.put_chain_state(chain).await?;

            sender_account.balance_x = sender_account
                .balance_x
                .checked_add(*amount)
                .and_then(|b| b.checked_sub(gas_fee))
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(gas_used, vec!["unstake".into()]))
        }
        TxPayload::Delegate { validator, amount } => {
            ensure_funds(&sender_account, *amount, gas_fee)?;
            let Some(v) = chain.validators.values_mut().find(|v| v.owner == *validator) else {
                anyhow::bail!("validator not found");
            };
            v.stake = v
                .stake
                .checked_add(*amount)
                .ok_or_else(|| anyhow::anyhow!("stake overflow"))?;
            chain.delegations.push(Delegation {
                delegator: sender,
                validator_id: v.id,
                stake: *amount,
            });
            ctx.state.put_chain_state(chain).await?;

            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(*amount + gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["delegate".into()],
            ))
        }
        TxPayload::Undelegate { validator, amount } => {
            let Some(v) = chain.validators.values_mut().find(|v| v.owner == *validator) else {
                anyhow::bail!("validator not found");
            };
            let mut found = false;
            for delegation in chain.delegations.iter_mut() {
                if delegation.delegator == sender && delegation.validator_id == v.id {
                    if delegation.stake < *amount {
                        anyhow::bail!("undelegate amount exceeds delegation");
                    }
                    delegation.stake -= *amount;
                    v.stake = v.stake.saturating_sub(*amount);
                    found = true;
                    break;
                }
            }
            if !found {
                anyhow::bail!("delegation not found");
            }
            chain.delegations.retain(|d| d.stake > 0);
            ctx.state.put_chain_state(chain).await?;

            sender_account.balance_x = sender_account
                .balance_x
                .checked_add(*amount)
                .and_then(|b| b.checked_sub(gas_fee))
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["undelegate".into()],
            ))
        }
        TxPayload::DomainCreate { domain_id, params } => {
            chain.domains.insert(
                *domain_id,
                state::DomainEntry {
                    domain_id: *domain_id,
                    kind: state::DomainType::Custom,
                    security_model: state::SecurityModel::SharedSecurity,
                    sequencer_binding: None,
                    bridge_contracts: vec![],
                    risk_params: params.clone(),
                },
            );
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["domain_create".into()],
            ))
        }
        TxPayload::DomainConfigUpdate { domain_id, params } => {
            if let Some(entry) = chain.domains.get_mut(domain_id) {
                entry.risk_params = params.clone();
            }
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["domain_config_update".into()],
            ))
        }
        TxPayload::RollupBatchCommit { domain_id, blob_id } => {
            chain.da_commitments.push(state::DACommitment {
                block_height: 0,
                da_root: [0u8; 32],
                blob_ids: vec![blob_id.clone()],
            });
            chain.domain_roots.insert(
                *domain_id,
                state::DomainRoot {
                    domain_id: *domain_id,
                    state_root: [0u8; 32],
                    da_root: [0u8; 32],
                    last_verified_epoch: 0,
                    proof_meta: serde_json::json!({"blob": blob_id}),
                },
            );
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["rollup_batch_commit".into()],
            ))
        }
        TxPayload::RollupBridgeDeposit { domain_id, amount } => {
            ensure_funds(&sender_account, *amount, gas_fee)?;
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(*amount + gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            chain.fee_pools.treasury = chain
                .fee_pools
                .treasury
                .saturating_add(*amount);
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["bridge_deposit".into()],
            ))
        }
        TxPayload::RollupBridgeWithdraw { amount, .. } => {
            sender_account.balance_x = sender_account
                .balance_x
                .checked_add(*amount)
                .and_then(|b| b.checked_sub(gas_fee))
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["bridge_withdraw".into()],
            ))
        }
        TxPayload::GovernanceProposal { payload } => {
            let id = Uuid::new_v4();
            chain.proposals.insert(
                id,
                state::Proposal {
                    id,
                    payload: payload.clone(),
                    kind: "general".into(),
                    status: "active".into(),
                    votes: serde_json::json!({}),
                    timers: serde_json::json!({}),
                },
            );
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["gov_proposal".into()],
            ))
        }
        TxPayload::GovernanceVote { proposal_id, support, weight } => {
            if let Some(p) = chain.proposals.get_mut(proposal_id) {
                let mut votes = p.votes.as_object().cloned().unwrap_or_default();
                votes.insert(
                    hex::encode(sender),
                    serde_json::json!({"support": support, "weight": weight}),
                );
                p.votes = serde_json::Value::Object(votes);
            }
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(gas_used, vec!["gov_vote".into()]))
        }
        TxPayload::PrivacyDeposit { commitment } => {
            let pool = chain
                .privacy_pools
                .entry("shielded".into())
                .or_insert(state::PrivacyPool {
                    merkle_root: [0u8; 32],
                    parameters: serde_json::json!({}),
                    nullifiers: vec![],
                });
            pool.nullifiers.push(*commitment);
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["privacy_deposit".into()],
            ))
        }
        TxPayload::PrivacyWithdraw { nullifier, recipient } => {
            let pool = chain
                .privacy_pools
                .entry("shielded".into())
                .or_insert(state::PrivacyPool {
                    merkle_root: [0u8; 32],
                    parameters: serde_json::json!({}),
                    nullifiers: vec![],
                });
            pool.nullifiers.push(*nullifier);
            let mut to_account = ctx.state.get_account(recipient).await?.unwrap_or(default_account(*recipient));
            to_account.balance_x = to_account
                .balance_x
                .checked_add(1)
                .and_then(|b| b.checked_sub(gas_fee))
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            ctx.state.put_account(to_account).await?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["privacy_withdraw".into()],
            ))
        }
        TxPayload::SystemUpgrade { module, version } => {
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            chain.proposals.insert(
                Uuid::new_v4(),
                state::Proposal {
                    id: Uuid::new_v4(),
                    payload: serde_json::json!({ "module": module, "version": version }),
                    kind: "upgrade".into(),
                    status: "queued".into(),
                    votes: serde_json::json!({}),
                    timers: serde_json::json!({}),
                },
            );
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["system_upgrade".into()],
            ))
        }
    }
}

pub async fn apply_block<S: StateStore>(
    ctx: &ExecutionContext<S>,
    block: &Block,
) -> anyhow::Result<BlockApplyResult> {
    let mut gas_used = 0_u64;
    let mut events = Vec::new();
    for tx in &block.transactions {
        let result = apply_tx(ctx, tx).await?;
        gas_used = gas_used.saturating_add(result.gas_used);
        events.extend(result.events);
        if gas_used > ctx.max_gas_per_block {
            anyhow::bail!("block exceeds gas limit");
        }
    }
    let state_root = ctx.state.commit().await?;
    Ok(BlockApplyResult {
        state_root,
        gas_used,
        events,
    })
}

#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    pub gas_used: u64,
    pub events: Vec<String>,
}

impl ExecutionOutcome {
    pub fn success(gas_used: u64, events: Vec<String>) -> Self {
        Self { gas_used, events }
    }
}

#[derive(Debug, Clone)]
pub struct BlockApplyResult {
    pub state_root: Hash,
    pub gas_used: u64,
    pub events: Vec<String>,
}

pub fn derive_sender(signature: &[u8]) -> Address {
    // Placeholder: replace with real recovery from signature.
    let mut addr = [0u8; 32];
    for (i, b) in signature.iter().take(32).enumerate() {
        addr[i] = *b;
    }
    addr
}

pub fn bootstrap_state() -> ExecutionContext<InMemoryStateStore> {
    let default_genesis = GenesisConfig {
        chain_id: "kova-devnet".into(),
        initial_validators: vec![],
        initial_accounts: vec![],
        block_time_ms: 1_000,
        max_gas_per_block: 30_000_000,
        base_fee: 1,
        da_sample_count: 8,
        slashing_double_sign: 5,
        fee_split: FeeSplit {
            l1_gas_burn_pct: 30,
            l1_gas_validators_pct: 70,
            da_validators_pct: 70,
            da_nodes_pct: 20,
            da_treasury_pct: 10,
            l2_sequencer_pct: 50,
            l2_da_costs_pct: 30,
            l2_l1_rent_pct: 20,
        },
    };
    futures::executor::block_on(from_genesis(default_genesis)).unwrap()
}

pub async fn from_genesis(
    genesis: GenesisConfig,
) -> anyhow::Result<ExecutionContext<InMemoryStateStore>> {
    let store = InMemoryStateStore::new();
    let mut chain = ChainState::default();

    for (address, balance) in genesis.initial_accounts {
        chain.accounts.insert(
            address,
            Account {
                address,
                nonce: 0,
                balance_x: balance,
                code_hash: None,
                storage_root: None,
            },
        );
    }

    for v in genesis.initial_validators {
        let id = Uuid::new_v4();
        chain.validators.insert(
            id,
            Validator {
                owner: derive_owner_from_pubkey(&v.pubkey),
                id,
                pubkey: v.pubkey.clone(),
                stake: v.stake,
                status: ValidatorStatus::Active,
                commission_rate: v.commission_rate,
            },
        );
    }

    chain.fee_pools = FeePools {
        l1_gas: 0,
        da: 0,
        sequencer: 0,
        treasury: 0,
    };

    store.put_chain_state(chain).await?;

    Ok(ExecutionContext::new(
        store,
        genesis.fee_split,
        genesis.chain_id,
        genesis.base_fee,
        genesis.max_gas_per_block,
        genesis.block_time_ms,
        genesis.da_sample_count,
        genesis.slashing_double_sign,
    ))
}

pub fn load_genesis_from_file(
    path: impl AsRef<Path>,
) -> anyhow::Result<ExecutionContext<InMemoryStateStore>> {
    let contents = fs::read_to_string(path)?;
    let genesis: GenesisConfig = serde_json::from_str(&contents)?;
    futures::executor::block_on(from_genesis(genesis))
}

fn default_account(address: Address) -> Account {
    Account {
        address,
        nonce: 0,
        balance_x: 0,
        code_hash: None,
        storage_root: None,
    }
}

fn gas_cost(payload: &TxPayload) -> u64 {
    match payload {
        TxPayload::Transfer { .. } => 21_000,
        TxPayload::Stake { .. } | TxPayload::Unstake { .. } => 50_000,
        TxPayload::Delegate { .. } | TxPayload::Undelegate { .. } => 60_000,
        _ => 50_000,
    }
}

fn effective_gas_price(tx: &Tx, base_fee: u128) -> anyhow::Result<u128> {
    if let Some(max_fee) = tx.max_fee {
        let priority = tx.max_priority_fee.unwrap_or(0);
        let total = base_fee
            .checked_add(priority)
            .ok_or_else(|| anyhow::anyhow!("fee overflow"))?;
        if max_fee < total {
            Ok(max_fee)
        } else {
            Ok(total)
        }
    } else if let Some(gas_price) = tx.gas_price {
        Ok(gas_price)
    } else {
        Ok(base_fee)
    }
}

fn ensure_funds(account: &Account, amount: u128, gas_fee: u128) -> anyhow::Result<()> {
    let total = amount
        .checked_add(gas_fee)
        .ok_or_else(|| anyhow::anyhow!("overflow"))?;
    if account.balance_x < total {
        anyhow::bail!("insufficient funds");
    }
    Ok(())
}

fn route_gas_fee(chain: &mut ChainState, gas_fee: u128, split: &FeeSplit) {
    let burn = gas_fee.saturating_mul(split.l1_gas_burn_pct as u128) / 100;
    let validators = gas_fee.saturating_mul(split.l1_gas_validators_pct as u128) / 100;
    chain.fee_pools.l1_gas = chain.fee_pools.l1_gas.saturating_add(validators);
    chain.fee_pools.treasury = chain.fee_pools.treasury.saturating_add(burn);
}

fn derive_owner_from_pubkey(pubkey: &[u8]) -> Address {
    let mut addr = [0u8; 32];
    for (i, b) in pubkey.iter().take(32).enumerate() {
        addr[i] = *b;
    }
    addr
}

pub fn hash_block(block: &Block) -> Hash {
    let bytes = bincode::serialize(block).unwrap_or_default();
    let digest = blake3::hash(&bytes);
    *digest.as_bytes()
}

