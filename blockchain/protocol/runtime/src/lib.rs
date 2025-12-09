use blake3;
use ed25519_dalek::{Signature, SigningKey, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
mod domains;
pub use domains::{
    CrossDomainMessage, DomainCall, DomainExecutionReceipt, DomainRuntime, FraudProof,
};
use state::{
    Account, ChainState, Delegation, FeePools, GovernanceParams, InMemoryStateStore, PrivacyPool,
    Proposal, ProposalStatus, StateStore, Unbonding, Validator, ValidatorStatus, VoteChoice,
    VoteRecord,
};
use std::fs;
use std::path::Path;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;
use zk_core::{Commitments, ProofArtifact, ZkBackend};
use zk_program_privacy;

pub type Address = [u8; 32];
pub type Hash = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TxPayload {
    Transfer { to: Address, amount: u128 },
    Stake { amount: u128 },
    Unstake { amount: u128 },
    Delegate { validator: Address, amount: u128 },
    Undelegate { validator: Address, amount: u128 },
    DomainExecute(DomainCall),
    CrossDomainSend {
        from_domain: Uuid,
        to_domain: Uuid,
        payload: serde_json::Value,
        fee: u128,
    },
    CrossDomainRelay { message: CrossDomainMessage },
    FraudChallenge {
        domain_id: Uuid,
        claimed_root: Hash,
        witness: serde_json::Value,
    },
    DomainCreate { domain_id: Uuid, params: serde_json::Value },
    DomainConfigUpdate { domain_id: Uuid, params: serde_json::Value },
    RollupBatchCommit { domain_id: Uuid, blob_id: String },
    RollupBridgeDeposit { domain_id: Uuid, amount: u128 },
    RollupBridgeWithdraw { domain_id: Uuid, amount: u128 },
    GovernanceProposal { payload: serde_json::Value, kind: Option<String> },
    GovernanceVote { proposal_id: Uuid, support: VoteChoice },
    GovernanceBridgeApprove { proposal_id: Uuid },
    GovernanceExecute { proposal_id: Uuid },
    Slash {
        validator: Address,
        penalty_bps: u16,
        reason: Option<String>,
    },
    PrivacyDeposit { commitment: Hash, amount: u128 },
    PrivacyWithdraw {
        nullifier: Hash,
        recipient: Address,
        amount: u128,
        merkle_root: Hash,
        commitment: Hash,
        proof: ProofArtifact,
    },
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
    pub public_key: Vec<u8>,
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
    pub da_commitment: Option<BlockDACommitment>,
    pub domain_roots: Vec<Hash>,
    pub gas_used: u64,
    pub gas_limit: u64,
    pub base_fee: u128,
    pub consensus_metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDACommitment {
    pub root: Hash,
    pub total_shards: u32,
    pub data_shards: u32,
    pub parity_shards: u32,
    pub shard_size: u32,
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
pub struct RewardParams {
    pub base_inflation_bps: u16,
    pub max_inflation_bps: u16,
    pub target_stake_bps: u16,
    pub treasury_pct: u8,
    pub proposer_bonus_pct: u8,
}

impl Default for RewardParams {
    fn default() -> Self {
        Self {
            base_inflation_bps: 500,   // 5% when at target or above
            max_inflation_bps: 1500,   // 15% when below target stake
            target_stake_bps: 6_700,   // 67% staked target
            treasury_pct: 10,
            proposer_bonus_pct: 5,
        }
    }
}

fn default_unbonding_delay_blocks() -> u64 {
    10
}

fn default_slash_penalty_bps() -> u16 {
    500
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
    #[serde(default)]
    pub initial_total_supply: u128,
    #[serde(default = "RewardParams::default")]
    pub reward_params: RewardParams,
    #[serde(default = "default_unbonding_delay_blocks")]
    pub unbonding_delay_blocks: u64,
    #[serde(default = "default_slash_penalty_bps")]
    pub slash_penalty_bps: u16,
}

#[derive(Clone)]
pub struct ExecutionContext<S: StateStore> {
    pub state: S,
    pub fee_split: FeeSplit,
    pub chain_id: String,
    pub base_fee: u128,
    pub max_gas_per_block: u64,
    pub block_time_ms: u64,
    pub da_sample_count: u16,
    pub slashing_double_sign: u8,
    pub reward_params: RewardParams,
    pub unbonding_delay_blocks: u64,
    pub slash_penalty_bps: u16,
    pub zk: Option<Arc<dyn ZkBackend>>,
    pub domains: Arc<DomainRuntime>,
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
        reward_params: RewardParams,
        unbonding_delay_blocks: u64,
        slash_penalty_bps: u16,
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
            reward_params,
            unbonding_delay_blocks,
            slash_penalty_bps,
            zk: None,
            domains: Arc::new(DomainRuntime::new()),
        }
    }

    pub fn with_zk(mut self, zk: Option<Arc<dyn ZkBackend>>) -> Self {
        self.zk = zk;
        self
    }

    pub fn with_domains(mut self, domains: Arc<DomainRuntime>) -> Self {
        self.domains = domains;
        self
    }
}

pub async fn apply_tx<S: StateStore>(
    ctx: &ExecutionContext<S>,
    tx: &Tx,
    current_height: u64,
) -> anyhow::Result<ExecutionOutcome> {
    let sender = verify_tx_signature(tx)?;
    if tx.chain_id != ctx.chain_id {
        anyhow::bail!("invalid chain id");
    }

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
            sync_accounts_from_store(ctx, &mut chain).await?;
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
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(gas_used, vec!["stake".into()]))
        }
        TxPayload::Unstake { amount } => {
            if sender_account.balance_x < gas_fee {
                anyhow::bail!("insufficient funds for gas");
            }
            let Some(v) = chain.validators.values_mut().find(|v| v.owner == sender) else {
                anyhow::bail!("no validator for sender");
            };
            if v.stake < *amount {
                anyhow::bail!("insufficient staked amount");
            }
            v.stake = v.stake.saturating_sub(*amount);
            if v.stake == 0 {
                v.status = ValidatorStatus::Exited;
            }
            let release_height = current_height.saturating_add(ctx.unbonding_delay_blocks);
            chain.pending_unbonds.push(Unbonding {
                owner: sender,
                validator_id: Some(v.id),
                amount: *amount,
                release_height,
            });
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(gas_used, vec!["unstake_init".into()]))
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
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(*amount + gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
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
            let release_height = current_height.saturating_add(ctx.unbonding_delay_blocks);
            chain.pending_unbonds.push(Unbonding {
                owner: sender,
                validator_id: Some(v.id),
                amount: *amount,
                release_height,
            });
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["undelegate_init".into()],
            ))
        }
        TxPayload::DomainExecute(call) => {
            let entry = chain
                .domains
                .get(&call.domain_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("domain not registered"))?;
            if !ctx.domains.has_domain(&call.domain_id) {
                ctx.domains.register(&entry)?;
            }
            if sender_account.balance_x < gas_fee {
                anyhow::bail!("insufficient funds for gas");
            }
            let receipt = ctx
                .domains
                .execute(call, ctx, current_height)
                .await
                .map_err(|e| anyhow::anyhow!("domain execution failed: {e}"))?;

            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);

            chain.domain_roots.insert(
                receipt.domain_id,
                state::DomainRoot {
                    domain_id: receipt.domain_id,
                    state_root: receipt.state_root,
                    da_root: [0u8; 32],
                    last_verified_epoch: current_height,
                    proof_meta: serde_json::json!({ "trace": receipt.trace }),
                },
            );
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            let mut events = receipt.events.clone();
            events.push("domain_execute".into());
            Ok(ExecutionOutcome::success(receipt.gas_used, events))
        }
        TxPayload::CrossDomainSend {
            from_domain,
            to_domain,
            payload,
            fee,
        } => {
            if sender_account.balance_x < gas_fee.saturating_add(*fee) {
                anyhow::bail!("insufficient funds for gas + fee");
            }
            let _ = chain
                .domains
                .get(from_domain)
                .ok_or_else(|| anyhow::anyhow!("from_domain not registered"))?;
            let _ = chain
                .domains
                .get(to_domain)
                .ok_or_else(|| anyhow::anyhow!("to_domain not registered"))?;
            let nonce = ctx.domains.next_out_nonce(from_domain);
            let msg = CrossDomainMessage {
                from: *from_domain,
                to: *to_domain,
                nonce,
                fee: *fee,
                payload: payload.clone(),
            };
            ctx.domains.push_outbox(msg);
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee.saturating_add(*fee))
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["cross_domain_send".into()],
            ))
        }
        TxPayload::CrossDomainRelay { message } => {
            ctx.domains.relay_message(message.clone())?;
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["cross_domain_relay".into()],
            ))
        }
        TxPayload::FraudChallenge {
            domain_id,
            claimed_root,
            witness,
        } => {
            let proof = FraudProof {
                domain_id: *domain_id,
                claimed_root: *claimed_root,
                witness: witness.clone(),
            };
            ctx.domains
                .submit_fraud_proof(&proof)
                .map_err(|e| anyhow::anyhow!("fraud proof rejected: {e}"))?;
            chain.domain_roots.insert(
                *domain_id,
                state::DomainRoot {
                    domain_id: *domain_id,
                    state_root: *claimed_root,
                    da_root: [0u8; 32],
                    last_verified_epoch: current_height,
                    proof_meta: serde_json::json!({ "fraud_proof": witness.clone() }),
                },
            );
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["fraud_challenge".into()],
            ))
        }
        TxPayload::DomainCreate { domain_id, params } => {
            validate_domain_risk(params)?;
            let kind = params
                .get("kind")
                .and_then(|v| v.as_str())
                .map(|s| match s.to_lowercase().as_str() {
                    "evm" | "evm_shared_security" => state::DomainType::EvmSharedSecurity,
                    "wasm" => state::DomainType::Wasm,
                    "privacy" => state::DomainType::Privacy,
                    "payment" => state::DomainType::Payment,
                    _ => state::DomainType::Custom,
                })
                .unwrap_or(state::DomainType::Custom);
            let entry = state::DomainEntry {
                domain_id: *domain_id,
                kind,
                security_model: state::SecurityModel::SharedSecurity,
                sequencer_binding: None,
                bridge_contracts: vec![],
                risk_params: params.clone(),
            };
            chain.domains.insert(*domain_id, entry.clone());
            let _ = ctx.domains.register(&entry);
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["domain_create".into()],
            ))
        }
        TxPayload::DomainConfigUpdate { domain_id, params } => {
            validate_domain_risk(params)?;
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
            sync_accounts_from_store(ctx, &mut chain).await?;
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
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["rollup_batch_commit".into()],
            ))
        }
        TxPayload::RollupBridgeDeposit { domain_id: _, amount } => {
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
            sync_accounts_from_store(ctx, &mut chain).await?;
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
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["bridge_withdraw".into()],
            ))
        }
        TxPayload::GovernanceProposal { payload, kind } => {
            let id = Uuid::new_v4();
            let now = now_millis();
            let voter_weights = snapshot_validator_weights(&chain);
            let snapshot_total_stake = voter_weights.values().copied().sum();
            let proposal = state::Proposal {
                id,
                payload: payload.clone(),
                kind: kind.clone().unwrap_or_else(|| "general".into()),
                status: ProposalStatus::Active,
                proposer: sender,
                start: now,
                end: now + chain.governance_params.voting_period_ms,
                eta: None,
                snapshot_total_stake,
                for_votes: 0,
                against_votes: 0,
                abstain_votes: 0,
                votes: Vec::new(),
                execution: payload.clone(),
                voter_weights,
                approvals: Vec::new(),
            };
            chain.proposals.insert(id, proposal);
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["gov_proposal".into()],
            ))
        }
        TxPayload::GovernanceVote { proposal_id, support } => {
            let Some(p) = chain.proposals.get_mut(proposal_id) else {
                anyhow::bail!("proposal not found");
            };
            if p.status != ProposalStatus::Active {
                anyhow::bail!("proposal not active");
            }
            let now = now_millis();
            if now > p.end {
                finalize_proposal(p, &chain.governance_params, now);
                anyhow::bail!("voting window closed");
            }
            if p.votes.iter().any(|v| v.voter == sender) {
                anyhow::bail!("already voted");
            }
            let weight = *p.voter_weights.get(&sender).unwrap_or(&0);
            if weight == 0 {
                anyhow::bail!("no voting power");
            }
            match support {
                VoteChoice::For => p.for_votes = p.for_votes.saturating_add(weight),
                VoteChoice::Against => p.against_votes = p.against_votes.saturating_add(weight),
                VoteChoice::Abstain => p.abstain_votes = p.abstain_votes.saturating_add(weight),
            }
            p.votes.push(VoteRecord {
                voter: sender,
                choice: support.clone(),
                weight,
            });
            finalize_proposal(p, &chain.governance_params, now);

            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(gas_used, vec!["gov_vote".into()]))
        }
        TxPayload::GovernanceBridgeApprove { proposal_id } => {
            let Some(p) = chain.proposals.get_mut(proposal_id) else {
                anyhow::bail!("proposal not found");
            };
            if !matches!(p.status, ProposalStatus::Queued | ProposalStatus::Succeeded) {
                anyhow::bail!("proposal not ready for bridge approval");
            }
            ensure_multisig_eligibility(&chain.governance_params, &sender)?;
            if p.approvals.contains(&sender) {
                anyhow::bail!("already approved");
            }
            p.approvals.push(sender);
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["gov_bridge_approve".into()],
            ))
        }
        TxPayload::GovernanceExecute { proposal_id } => {
            let Some(p) = chain.proposals.get_mut(proposal_id) else {
                anyhow::bail!("proposal not found");
            };
            let now = now_millis();
            finalize_proposal(p, &chain.governance_params, now);
            if p.status != ProposalStatus::Queued {
                anyhow::bail!("proposal not queued for execution");
            }
            if let Some(eta) = p.eta {
                if now < eta {
                    anyhow::bail!("timelock not satisfied");
                }
            } else {
                anyhow::bail!("missing eta");
            }
            ensure_multisig_threshold_met(&chain.governance_params, &p.approvals)?;
            p.status = ProposalStatus::Executed;

            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["gov_execute".into()],
            ))
        }
        TxPayload::Slash {
            validator,
            penalty_bps,
            reason: _,
        } => {
            let Some(v) = chain.validators.values_mut().find(|v| v.owner == *validator) else {
                anyhow::bail!("validator not found");
            };
            let stake_before = v.stake;
            if stake_before == 0 {
                anyhow::bail!("validator has no stake to slash");
            }
            let effective_bps = if *penalty_bps == 0 {
                ctx.slash_penalty_bps
            } else {
                *penalty_bps
            }
            .min(10_000);
            let penalty = stake_before
                .saturating_mul(effective_bps as u128)
                / 10_000;
            if penalty == 0 {
                anyhow::bail!("penalty too small");
            }

            if stake_before > 0 && penalty > 0 && !chain.delegations.is_empty() {
                let mut updated = Vec::with_capacity(chain.delegations.len());
                for mut d in chain.delegations.drain(..) {
                    if d.validator_id == v.id {
                        let cut = penalty.saturating_mul(d.stake) / stake_before;
                        d.stake = d.stake.saturating_sub(cut);
                    }
                    if d.stake > 0 {
                        updated.push(d);
                    }
                }
                chain.delegations = updated;
            }

            v.stake = v.stake.saturating_sub(penalty);
            if v.stake == 0 {
                v.status = ValidatorStatus::Jailed;
            }
            chain.fee_pools.treasury = chain.fee_pools.treasury.saturating_add(penalty);

            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(gas_used, vec!["slash".into()]))
        }
        TxPayload::PrivacyDeposit { commitment, amount } => {
            ensure_positive(*amount)?;
            ensure_funds(&sender_account, *amount, gas_fee)?;
            let pool = ensure_privacy_pool(&mut chain);
            if pool.commitments.contains(commitment) {
                anyhow::bail!("commitment already exists in pool");
            }
            pool.commitments.push(*commitment);
            pool.total_shielded = pool
                .total_shielded
                .checked_add(*amount)
                .ok_or_else(|| anyhow::anyhow!("shielded total overflow"))?;
            pool.merkle_root = compute_merkle_root(&pool.commitments);

            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(*amount + gas_fee)
                .ok_or_else(|| anyhow::anyhow!("underflow"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
            ctx.state.put_chain_state(chain).await?;
            Ok(ExecutionOutcome::success(
                gas_used,
                vec!["privacy_deposit".into()],
            ))
        }
        TxPayload::PrivacyWithdraw {
            nullifier,
            recipient,
            amount,
            merkle_root,
            commitment,
            proof,
        } => {
            ensure_positive(*amount)?;
            let pool = ensure_privacy_pool(&mut chain);
            if pool.nullifiers.contains(nullifier) {
                anyhow::bail!("nullifier already spent");
            }
            if &pool.merkle_root != merkle_root {
                anyhow::bail!("merkle root mismatch");
            }
            if !pool.commitments.contains(commitment) {
                anyhow::bail!("commitment not found in pool");
            }
            if pool.total_shielded < *amount {
                anyhow::bail!("insufficient shielded liquidity");
            }

            let input = zk_program_privacy::PrivacyWithdrawInput {
                nullifier: *nullifier,
                merkle_root: *merkle_root,
                recipient: *recipient,
                amount: *amount,
                commitment: *commitment,
            };
            verify_privacy_withdraw(ctx, &input, proof).await?;

            pool.nullifiers.push(*nullifier);
            pool.total_shielded = pool.total_shielded.saturating_sub(*amount);
            let mut to_account =
                ctx.state.get_account(recipient).await?.unwrap_or(default_account(*recipient));
            to_account.balance_x = to_account
                .balance_x
                .checked_add(*amount)
                .ok_or_else(|| anyhow::anyhow!("overflow"))?;
            ctx.state.put_account(to_account).await?;
            sender_account.balance_x = sender_account
                .balance_x
                .checked_sub(gas_fee)
                .ok_or_else(|| anyhow::anyhow!("insufficient funds for gas"))?;
            sender_account.nonce += 1;
            ctx.state.put_account(sender_account).await?;
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
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
            let now = now_millis();
            let id = Uuid::new_v4();
            chain.proposals.insert(
                id,
                state::Proposal {
                    id,
                    payload: serde_json::json!({ "module": module, "version": version }),
                    kind: "upgrade".into(),
                    status: ProposalStatus::Queued,
                    proposer: sender,
                    start: now,
                    end: now,
                    eta: Some(now + chain.governance_params.timelock_ms),
                    snapshot_total_stake: 0,
                    for_votes: 0,
                    against_votes: 0,
                    abstain_votes: 0,
                    votes: Vec::new(),
                    execution: serde_json::json!({ "module": module, "version": version }),
                    voter_weights: HashMap::new(),
                    approvals: Vec::new(),
                },
            );
            route_gas_fee(&mut chain, gas_fee, &ctx.fee_split);
            sync_accounts_from_store(ctx, &mut chain).await?;
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
        let result = apply_tx(ctx, tx, block.header.height).await?;
        gas_used = gas_used.saturating_add(result.gas_used);
        events.extend(result.events);
        if gas_used > ctx.max_gas_per_block {
            anyhow::bail!("block exceeds gas limit");
        }
    }
    process_unbondings(ctx, block.header.height).await?;
    let minted = apply_inflation_rewards(ctx, block).await?;
    if minted > 0 {
        events.push("block_reward".into());
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
        initial_total_supply: 0,
        reward_params: RewardParams::default(),
        unbonding_delay_blocks: default_unbonding_delay_blocks(),
        slash_penalty_bps: default_slash_penalty_bps(),
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
        let id = validator_id_from_pubkey(&v.pubkey);
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

    let mut computed_supply: u128 = genesis.initial_total_supply;
    if computed_supply == 0 {
        computed_supply = chain.accounts.values().map(|a| a.balance_x).sum();
        let validator_stake: u128 = chain.validators.values().map(|v| v.stake).sum();
        computed_supply = computed_supply.saturating_add(validator_stake);
    }
    chain.total_supply = computed_supply;
    chain.last_reward_height = 0;

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
        genesis.reward_params,
        genesis.unbonding_delay_blocks,
        genesis.slash_penalty_bps,
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
        TxPayload::Slash { .. } => 70_000,
        TxPayload::PrivacyDeposit { .. } => 80_000,
        TxPayload::PrivacyWithdraw { .. } => 120_000,
        TxPayload::GovernanceExecute { .. } => 80_000,
        TxPayload::DomainExecute(_) => 200_000,
        TxPayload::CrossDomainSend { .. } => 90_000,
        TxPayload::CrossDomainRelay { .. } => 50_000,
        TxPayload::FraudChallenge { .. } => 150_000,
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
    address_from_pubkey(pubkey)
}

fn validator_id_from_pubkey(pubkey: &[u8]) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, pubkey)
}

pub fn hash_block(block: &Block) -> Hash {
    let bytes = bincode::serialize(block).unwrap_or_default();
    let digest = blake3::hash(&bytes);
    *digest.as_bytes()
}

pub fn address_from_pubkey(pubkey: &[u8]) -> Address {
    let digest = blake3::hash(pubkey);
    *digest.as_bytes()
}

pub fn sign_bytes(signing_key: &SigningKey, msg: &[u8]) -> Vec<u8> {
    signing_key.sign(msg).to_bytes().to_vec()
}

pub fn verify_signature_bytes(
    public_key: &[u8],
    signature: &[u8],
    msg: &[u8],
) -> anyhow::Result<()> {
    let pk_bytes: &[u8; 32] = public_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid pubkey length"))?;
    let sig_bytes: &[u8; 64] = signature
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid signature length"))?;
    let vk = VerifyingKey::from_bytes(pk_bytes)?;
    let sig = Signature::from_bytes(sig_bytes);
    vk.verify(msg, &sig)?;
    Ok(())
}

pub fn tx_signing_bytes(tx: &Tx) -> anyhow::Result<Vec<u8>> {
    let signable = (
        &tx.chain_id,
        tx.nonce,
        tx.gas_limit,
        tx.max_fee,
        tx.max_priority_fee,
        tx.gas_price,
        &tx.payload,
        &tx.public_key,
    );
    Ok(bincode::serialize(&signable)?)
}

pub fn verify_tx_signature(tx: &Tx) -> anyhow::Result<Address> {
    let msg = tx_signing_bytes(tx)?;
    verify_signature_bytes(&tx.public_key, &tx.signature, &msg)?;
    Ok(address_from_pubkey(&tx.public_key))
}

fn ensure_positive(amount: u128) -> anyhow::Result<()> {
    if amount == 0 {
        anyhow::bail!("amount must be > 0");
    }
    Ok(())
}

fn ensure_privacy_pool<'a>(chain: &'a mut ChainState) -> &'a mut PrivacyPool {
    chain
        .privacy_pools
        .entry("shielded".into())
        .or_insert_with(PrivacyPool::default)
}

fn compute_merkle_root(commitments: &[Hash]) -> Hash {
    if commitments.is_empty() {
        return [0u8; 32];
    }
    let mut leaves: Vec<Hash> = commitments
        .iter()
        .map(|c| *blake3::hash(c).as_bytes())
        .collect();
    leaves.sort();
    let mut hasher = blake3::Hasher::new();
    for leaf in leaves {
        hasher.update(&leaf);
    }
    *hasher.finalize().as_bytes()
}

fn snapshot_validator_weights(chain: &ChainState) -> HashMap<Address, u128> {
    let mut weights = HashMap::new();
    for v in chain.validators.values() {
        weights.insert(v.owner, v.stake);
    }
    weights
}

fn quorum_met(p: &Proposal, params: &GovernanceParams) -> bool {
    let participated = p.for_votes.saturating_add(p.against_votes).saturating_add(p.abstain_votes);
    if p.snapshot_total_stake == 0 {
        return false;
    }
    participated.saturating_mul(10_000) >= params.quorum_bps as u128 * p.snapshot_total_stake
}

fn approval_met(p: &Proposal, params: &GovernanceParams) -> bool {
    let total = p.for_votes.saturating_add(p.against_votes).saturating_add(p.abstain_votes);
    if total == 0 {
        return false;
    }
    p.for_votes.saturating_mul(10_000) >= params.approval_threshold_bps as u128 * total
}

fn finalize_proposal(p: &mut Proposal, params: &GovernanceParams, now: u64) {
    if p.status != ProposalStatus::Active {
        return;
    }
    if now < p.end {
        return;
    }
    if !quorum_met(p, params) {
        p.status = ProposalStatus::Defeated;
        return;
    }
    if approval_met(p, params) {
        p.status = ProposalStatus::Queued;
        p.eta = Some(now.saturating_add(params.timelock_ms));
    } else {
        p.status = ProposalStatus::Defeated;
    }
}

fn ensure_multisig_eligibility(params: &GovernanceParams, signer: &Address) -> anyhow::Result<()> {
    if params.multisig_signers.is_empty() {
        return Ok(());
    }
    if !params.multisig_signers.contains(signer) {
        anyhow::bail!("sender not authorized for multisig bridge");
    }
    Ok(())
}

fn ensure_multisig_threshold_met(
    params: &GovernanceParams,
    approvals: &[Address],
) -> anyhow::Result<()> {
    if params.multisig_signers.is_empty() {
        return Ok(());
    }
    let unique: HashSet<_> = approvals.iter().copied().collect();
    if unique.len() < params.multisig_threshold as usize {
        anyhow::bail!("multisig threshold not met");
    }
    Ok(())
}

fn validate_domain_risk(params: &serde_json::Value) -> anyhow::Result<()> {
    if let Some(bps) = params.get("max_loss_bps").and_then(|v| v.as_u64()) {
        if bps > 10_000 {
            anyhow::bail!("max_loss_bps must be <= 10000");
        }
    }
    if let Some(cap) = params.get("risk_cap").and_then(|v| v.as_u64()) {
        if cap == 0 {
            anyhow::bail!("risk_cap must be > 0");
        }
    }
    Ok(())
}

fn commitments_equal(a: &Option<Commitments>, b: &Option<Commitments>) -> bool {
    match (a, b) {
        (Some(left), Some(right)) => {
            bincode::serialize(left).ok() == bincode::serialize(right).ok()
        }
        (None, None) => true,
        _ => false,
    }
}

async fn sync_accounts_from_store<S: StateStore>(
    ctx: &ExecutionContext<S>,
    chain: &mut ChainState,
) -> anyhow::Result<()> {
    let current = ctx.state.get_chain_state().await?;
    chain.accounts = current.accounts;
    Ok(())
}

async fn verify_privacy_withdraw<S: StateStore>(
    ctx: &ExecutionContext<S>,
    input: &zk_program_privacy::PrivacyWithdrawInput,
    artifact: &ProofArtifact,
) -> anyhow::Result<()> {
    if artifact.program_id != zk_program_privacy::program_id() {
        anyhow::bail!("invalid proof program id");
    }
    let commitments = zk_program_privacy::commitments(input);
    if !commitments_equal(&artifact.commitments, &Some(commitments.clone())) {
        anyhow::bail!("proof commitments mismatch");
    }

    if let Some(zk) = ctx.zk.clone() {
        zk.verify(artifact)
            .await
            .map_err(|e| anyhow::anyhow!("zk verification failed: {e}"))?;
        return Ok(());
    }

    if artifact.backend == "stub" {
        zk_program_privacy::verify_stub_artifact(artifact, input)?;
        return Ok(());
    }

    anyhow::bail!("no zk backend configured for privacy verification")
}

fn total_bonded_stake(chain: &ChainState) -> u128 {
    chain.validators.values().map(|v| v.stake).sum()
}

fn blocks_per_year(block_time_ms: u64) -> u128 {
    let ms_per_year: u128 = 365 * 24 * 60 * 60 * 1_000;
    let denom = block_time_ms.max(1) as u128;
    (ms_per_year / denom).max(1)
}

fn current_inflation_bps(chain: &ChainState, params: &RewardParams) -> u16 {
    let supply = chain.total_supply.max(1);
    let staked = total_bonded_stake(chain);
    if staked == 0 {
        return params.max_inflation_bps;
    }
    let ratio = (staked.min(supply).saturating_mul(10_000) / supply) as u16;
    if ratio >= params.target_stake_bps {
        params.base_inflation_bps
    } else {
        params.max_inflation_bps
    }
}

fn add_payout(payouts: &mut HashMap<Address, u128>, address: Address, amount: u128) {
    if amount == 0 {
        return;
    }
    let entry = payouts.entry(address).or_insert(0);
    *entry = entry.saturating_add(amount);
}

async fn credit_payouts<S: StateStore>(
    ctx: &ExecutionContext<S>,
    payouts: HashMap<Address, u128>,
) -> anyhow::Result<()> {
    for (address, amount) in payouts {
        if amount == 0 {
            continue;
        }
        let mut account = ctx
            .state
            .get_account(&address)
            .await?
            .unwrap_or(default_account(address));
        account.balance_x = account
            .balance_x
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("balance overflow"))?;
        ctx.state.put_account(account).await?;
    }
    Ok(())
}

async fn process_unbondings<S: StateStore>(
    ctx: &ExecutionContext<S>,
    current_height: u64,
) -> anyhow::Result<()> {
    let mut chain = ctx.state.get_chain_state().await?;
    if chain.pending_unbonds.is_empty() {
        return Ok(());
    }
    let mut remaining = Vec::with_capacity(chain.pending_unbonds.len());
    for entry in chain.pending_unbonds.drain(..) {
        if entry.release_height > current_height {
            remaining.push(entry);
            continue;
        }
        let mut account = ctx
            .state
            .get_account(&entry.owner)
            .await?
            .unwrap_or(default_account(entry.owner));
        account.balance_x = account
            .balance_x
            .checked_add(entry.amount)
            .ok_or_else(|| anyhow::anyhow!("balance overflow"))?;
        ctx.state.put_account(account).await?;
    }
    chain.pending_unbonds = remaining;
    sync_accounts_from_store(ctx, &mut chain).await?;
    ctx.state.put_chain_state(chain).await?;
    Ok(())
}

async fn apply_inflation_rewards<S: StateStore>(
    ctx: &ExecutionContext<S>,
    block: &Block,
) -> anyhow::Result<u128> {
    let mut chain = ctx.state.get_chain_state().await?;
    let total_stake = total_bonded_stake(&chain);
    if total_stake == 0 {
        return Ok(0);
    }
    let inflation_bps = current_inflation_bps(&chain, &ctx.reward_params);
    let blocks_per_year = blocks_per_year(ctx.block_time_ms);
    let mint = chain
        .total_supply
        .saturating_mul(inflation_bps as u128)
        / 10_000
        / blocks_per_year;
    if mint == 0 {
        return Ok(0);
    }

    let mut payouts: HashMap<Address, u128> = HashMap::new();
    let treasury = mint.saturating_mul(ctx.reward_params.treasury_pct as u128) / 100;
    chain.fee_pools.treasury = chain.fee_pools.treasury.saturating_add(treasury);
    let mut distributable = mint.saturating_sub(treasury);

    let proposer_bonus =
        distributable.saturating_mul(ctx.reward_params.proposer_bonus_pct as u128) / 100;
    if proposer_bonus > 0 {
        add_payout(&mut payouts, block.header.proposer_id, proposer_bonus);
        distributable = distributable.saturating_sub(proposer_bonus);
    }

    if distributable > 0 {
        for v in chain.validators.values() {
            if v.stake == 0 {
                continue;
            }
            let share = distributable.saturating_mul(v.stake) / total_stake;
            if share == 0 {
                continue;
            }

            let delegated_total: u128 = chain
                .delegations
                .iter()
                .filter(|d| d.validator_id == v.id)
                .map(|d| d.stake)
                .sum();
            let self_stake = v.stake.saturating_sub(delegated_total);

            let delegated_reward = if v.stake > 0 {
                share.saturating_mul(delegated_total) / v.stake
            } else {
                0
            };
            let commission = delegated_reward
                .saturating_mul(v.commission_rate as u128)
                / 100;
            let delegator_pool = delegated_reward.saturating_sub(commission);

            let self_reward = if v.stake > 0 {
                share.saturating_mul(self_stake) / v.stake
            } else {
                share
            };
            let validator_reward = self_reward.saturating_add(commission);
            add_payout(&mut payouts, v.owner, validator_reward);

            if delegated_total > 0 && delegator_pool > 0 {
                for delegation in chain.delegations.iter().filter(|d| d.validator_id == v.id) {
                    let reward = delegator_pool.saturating_mul(delegation.stake) / delegated_total;
                    add_payout(&mut payouts, delegation.delegator, reward);
                }
            }
        }
    }

    credit_payouts(ctx, payouts).await?;
    sync_accounts_from_store(ctx, &mut chain).await?;
    chain.total_supply = chain.total_supply.saturating_add(mint);
    chain.last_reward_height = block.header.height;
    ctx.state.put_chain_state(chain).await?;
    Ok(mint)
}

fn now_millis() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    now.as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use tokio::runtime::Runtime as TokioRuntime;
    use state::Account;

    fn signer() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn recipient_signer() -> SigningKey {
        SigningKey::from_bytes(&[9u8; 32])
    }

    fn default_genesis() -> GenesisConfig {
        let sk = signer();
        let pk = sk.verifying_key().to_bytes().to_vec();
        let addr = address_from_pubkey(&pk);
        GenesisConfig {
            chain_id: "kova-devnet".into(),
            initial_validators: vec![],
            initial_accounts: vec![(addr, 1_000_000)],
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
            initial_total_supply: 0,
            reward_params: RewardParams::default(),
            unbonding_delay_blocks: default_unbonding_delay_blocks(),
            slash_penalty_bps: default_slash_penalty_bps(),
        }
    }

    fn build_tx(payload: TxPayload, sk: &SigningKey, nonce: u64) -> Tx {
        let pk = sk.verifying_key().to_bytes().to_vec();
        let mut tx = Tx {
            chain_id: "kova-devnet".into(),
            nonce,
            gas_limit: 200_000,
            max_fee: Some(1),
            max_priority_fee: Some(0),
            gas_price: None,
            payload,
            public_key: pk.clone(),
            signature: vec![],
        };
        let msg = tx_signing_bytes(&tx).unwrap();
        tx.signature = sign_bytes(sk, &msg);
        tx
    }

    #[test]
    fn privacy_deposit_and_withdraw_stub() {
        let rt = TokioRuntime::new().unwrap();
        rt.block_on(async {
            let sk = signer();
            let recipient_sk = recipient_signer();
            let recipient_addr = address_from_pubkey(&recipient_sk.verifying_key().to_bytes());

            let ctx = from_genesis(default_genesis()).await.unwrap();

            let salt = [1u8; 32];
            let nullifier = [2u8; 32];
            let commitment =
                zk_program_privacy::note_commitment(&nullifier, &recipient_addr, 10, &salt);

            let deposit_tx = build_tx(
                TxPayload::PrivacyDeposit {
                    commitment,
                    amount: 10,
                },
                &sk,
                0,
            );
            apply_tx(&ctx, &deposit_tx, 0).await.unwrap();

            let chain = ctx.state.get_chain_state().await.unwrap();
            let pool = chain.privacy_pools.get("shielded").cloned().unwrap();
            assert_eq!(pool.total_shielded, 10);
            assert!(pool.commitments.contains(&commitment));
            let sender_addr = address_from_pubkey(&sk.verifying_key().to_bytes());
            let sender_after = ctx
                .state
                .get_account(&sender_addr)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(sender_after.nonce, 1);

            let input = zk_program_privacy::PrivacyWithdrawInput {
                nullifier,
                merkle_root: pool.merkle_root,
                recipient: recipient_addr,
                amount: 10,
                commitment,
            };
            let proof = zk_program_privacy::stub_withdraw_proof(&input).unwrap();

            let withdraw_tx = build_tx(
                TxPayload::PrivacyWithdraw {
                    nullifier,
                    recipient: recipient_addr,
                    amount: 10,
                    merkle_root: pool.merkle_root,
                    commitment,
                    proof,
                },
                &sk,
                sender_after.nonce,
            );
            apply_tx(&ctx, &withdraw_tx, 1).await.unwrap();

            let chain_after = ctx.state.get_chain_state().await.unwrap();
            let pool_after = chain_after.privacy_pools.get("shielded").cloned().unwrap();
            assert_eq!(pool_after.total_shielded, 0);
            assert!(pool_after.nullifiers.contains(&nullifier));
            let recipient_account = ctx
                .state
                .get_account(&recipient_addr)
                .await
                .unwrap()
                .unwrap();
            assert!(recipient_account.balance_x >= 10);

            // second withdraw should fail (nullifier spent)
            let proof2 = zk_program_privacy::stub_withdraw_proof(&input).unwrap();
            let double_spend_tx = build_tx(
                TxPayload::PrivacyWithdraw {
                    nullifier,
                    recipient: recipient_addr,
                    amount: 10,
                    merkle_root: pool.merkle_root,
                    commitment,
                    proof: proof2,
                },
                &sk,
                2,
            );
            assert!(apply_tx(&ctx, &double_spend_tx, 2).await.is_err());
        });
    }

    #[test]
    fn unstake_uses_unbonding_delay() {
        let rt = TokioRuntime::new().unwrap();
        rt.block_on(async {
            let sk = signer();
            let pk = sk.verifying_key().to_bytes().to_vec();
            let owner = address_from_pubkey(&pk);

            let mut ctx = bootstrap_state();
            ctx.unbonding_delay_blocks = 1;
            ctx.state
                .put_account(Account {
                    address: owner,
                    nonce: 0,
                    balance_x: 1_000_000,
                    code_hash: None,
                    storage_root: None,
                })
                .await
                .unwrap();

            let stake_tx = build_tx(TxPayload::Stake { amount: 100_000 }, &sk, 0);
            let stake_block = Block {
                header: BlockHeader {
                    parent_hash: [0u8; 32],
                    height: 0,
                    timestamp: 0,
                    proposer_id: owner,
                    state_root: [0u8; 32],
                    l1_tx_root: [0u8; 32],
                    da_commitment: None,
                    domain_roots: vec![],
                    gas_used: 0,
                    gas_limit: 30_000_000,
                    base_fee: 1,
                    consensus_metadata: serde_json::json!({}),
                },
                transactions: vec![stake_tx],
                da_blobs: vec![],
            };
            apply_block(&ctx, &stake_block).await.unwrap();

            let unstake_tx = build_tx(TxPayload::Unstake { amount: 50_000 }, &sk, 1);
            let unstake_block = Block {
                header: BlockHeader {
                    parent_hash: [0u8; 32],
                    height: 1,
                    timestamp: 0,
                    proposer_id: owner,
                    state_root: [0u8; 32],
                    l1_tx_root: [0u8; 32],
                    da_commitment: None,
                    domain_roots: vec![],
                    gas_used: 0,
                    gas_limit: 30_000_000,
                    base_fee: 1,
                    consensus_metadata: serde_json::json!({}),
                },
                transactions: vec![unstake_tx],
                da_blobs: vec![],
            };
            apply_block(&ctx, &unstake_block).await.unwrap();

            let chain = ctx.state.get_chain_state().await.unwrap();
            assert_eq!(chain.pending_unbonds.len(), 1);

            let before_release = ctx.state.get_account(&owner).await.unwrap().unwrap();
            assert!(before_release.balance_x < 850_000);

            let release_block = Block {
                header: BlockHeader {
                    parent_hash: [0u8; 32],
                    height: 2,
                    timestamp: 0,
                    proposer_id: owner,
                    state_root: [0u8; 32],
                    l1_tx_root: [0u8; 32],
                    da_commitment: None,
                    domain_roots: vec![],
                    gas_used: 0,
                    gas_limit: 30_000_000,
                    base_fee: 1,
                    consensus_metadata: serde_json::json!({}),
                },
                transactions: vec![],
                da_blobs: vec![],
            };
            apply_block(&ctx, &release_block).await.unwrap();
            let after_release = ctx.state.get_account(&owner).await.unwrap().unwrap();
            assert!(after_release.balance_x >= 850_000 - 2);
        });
    }
}

