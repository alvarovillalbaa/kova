use async_trait::async_trait;
use bigdecimal::BigDecimal;
use serde_json;
use runtime::{derive_sender, hash_block, Block, Tx, TxPayload};
use sqlx::{postgres::PgPoolOptions, Pool, Postgres};
use tracing::info;
use uuid::Uuid;

/// Generic sink for block ingestion.
#[async_trait]
pub trait BlockSink {
    async fn ingest_block(&mut self, block: Block) -> anyhow::Result<()>;
}

/// In-memory sink for tests and smoke runs.
#[derive(Default)]
pub struct InMemorySink {
    pub blocks: Vec<Block>,
}

#[async_trait]
impl BlockSink for InMemorySink {
    async fn ingest_block(&mut self, block: Block) -> anyhow::Result<()> {
        info!("ingesting block {}", block.header.height);
        self.blocks.push(block);
        Ok(())
    }
}

/// Postgres-backed sink that runs migrations and stores blocks/txs.
pub struct PostgresSink {
    pool: Pool<Postgres>,
}

impl PostgresSink {
    pub async fn connect(database_url: &str, max_connections: u32) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }
}

#[async_trait]
impl BlockSink for PostgresSink {
    async fn ingest_block(&mut self, block: Block) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        let block_hash = hash_block(&block);
        let height = i64::try_from(block.header.height)?;
        let timestamp = i64::try_from(block.header.timestamp)?;
        let gas_used = i64::try_from(block.header.gas_used)?;
        let gas_limit = i64::try_from(block.header.gas_limit)?;
        let domain_roots = serde_json::to_value(&block.header.domain_roots)?;
        let da_blobs = serde_json::to_value(&block.da_blobs)?;

        sqlx::query!(
            r#"
            INSERT INTO blocks (
                height, hash, parent_hash, timestamp_ms, proposer, state_root, l1_tx_root,
                da_root, domain_roots, gas_used, gas_limit, base_fee, tx_count,
                da_blobs, consensus_metadata
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
            ON CONFLICT (height) DO UPDATE SET
                hash = EXCLUDED.hash,
                parent_hash = EXCLUDED.parent_hash,
                timestamp_ms = EXCLUDED.timestamp_ms,
                proposer = EXCLUDED.proposer,
                state_root = EXCLUDED.state_root,
                l1_tx_root = EXCLUDED.l1_tx_root,
                da_root = EXCLUDED.da_root,
                domain_roots = EXCLUDED.domain_roots,
                gas_used = EXCLUDED.gas_used,
                gas_limit = EXCLUDED.gas_limit,
                base_fee = EXCLUDED.base_fee,
                tx_count = EXCLUDED.tx_count,
                da_blobs = EXCLUDED.da_blobs,
                consensus_metadata = EXCLUDED.consensus_metadata
            "#,
            height,
            block_hash.to_vec(),
            block.header.parent_hash.to_vec(),
            timestamp,
            block.header.proposer_id.to_vec(),
            block.header.state_root.to_vec(),
            block.header.l1_tx_root.to_vec(),
            block.header.da_root.to_vec(),
            domain_roots,
            gas_used,
            gas_limit,
            BigDecimal::from(block.header.base_fee),
            block.transactions.len() as i32,
            da_blobs,
            block.header.consensus_metadata
        )
        .execute(&mut *tx)
        .await?;

        for (position, tx_obj) in block.transactions.iter().enumerate() {
            ingest_tx(&mut tx, tx_obj, height, position as i32, block.header.height).await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

async fn ingest_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    raw_tx: &Tx,
    block_height: i64,
    position: i32,
    block_height_u64: u64,
) -> anyhow::Result<()> {
    let tx_hash = tx_hash(raw_tx);
    let sender = derive_sender(&raw_tx.signature);
    let payload_kind = payload_kind(&raw_tx.payload);
    let payload = serde_json::to_value(&raw_tx.payload)?;
    let events = payload_events(&raw_tx.payload);

    let rec = sqlx::query!(
        r#"
        INSERT INTO transactions (
            tx_hash, block_height, position, chain_id, sender, nonce, gas_limit,
            gas_price, max_fee, max_priority_fee, payload_type, payload, signature, events
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
        ON CONFLICT (tx_hash) DO UPDATE SET
            block_height = EXCLUDED.block_height,
            position = EXCLUDED.position,
            chain_id = EXCLUDED.chain_id,
            sender = EXCLUDED.sender,
            nonce = EXCLUDED.nonce,
            gas_limit = EXCLUDED.gas_limit,
            gas_price = EXCLUDED.gas_price,
            max_fee = EXCLUDED.max_fee,
            max_priority_fee = EXCLUDED.max_priority_fee,
            payload_type = EXCLUDED.payload_type,
            payload = EXCLUDED.payload,
            signature = EXCLUDED.signature,
            events = EXCLUDED.events
        RETURNING id
        "#,
        tx_hash.to_vec(),
        block_height,
        position,
        raw_tx.chain_id.to_string(),
        sender.to_vec(),
        i64::try_from(raw_tx.nonce)?,
        i64::try_from(raw_tx.gas_limit)?,
        raw_tx.gas_price.map(BigDecimal::from),
        raw_tx.max_fee.map(BigDecimal::from),
        raw_tx.max_priority_fee.map(BigDecimal::from),
        payload_kind,
        payload,
        raw_tx.signature.clone(),
        &events[..]
    )
    .fetch_one(&mut **tx)
    .await?;

    let tx_id = rec.id;
    touch_account(tx, &sender, block_height).await?;
    handle_payload(tx, tx_id, block_height_u64, &raw_tx.payload).await?;
    Ok(())
}

async fn handle_payload(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    tx_id: i64,
    block_height: u64,
    payload: &TxPayload,
) -> anyhow::Result<()> {
    let height = i64::try_from(block_height)?;
    match payload {
        TxPayload::Transfer { to, .. } => {
            touch_account(tx, to, height).await?;
        }
        TxPayload::Delegate { validator, .. } | TxPayload::Undelegate { validator, .. } => {
            touch_account(tx, validator, height).await?;
        }
        TxPayload::DomainCreate { domain_id, params } => {
            upsert_domain(tx, domain_id, params.clone()).await?;
        }
        TxPayload::DomainConfigUpdate { domain_id, params } => {
            upsert_domain(tx, domain_id, params.clone()).await?;
        }
        TxPayload::RollupBatchCommit { domain_id, blob_id } => {
            sqlx::query!(
                r#"
                INSERT INTO rollup_batches (domain_id, blob_id, block_height, tx_id)
                VALUES ($1,$2,$3,$4)
                "#,
                domain_id,
                blob_id,
                height,
                tx_id
            )
            .execute(&mut **tx)
            .await?;
        }
        TxPayload::GovernanceProposal { payload } => {
            sqlx::query!(
                r#"
                INSERT INTO governance_events (tx_id, kind, payload)
                VALUES ($1,'proposal',$2)
                "#,
                tx_id,
                payload
            )
            .execute(&mut **tx)
            .await?;
        }
        TxPayload::GovernanceVote {
            proposal_id,
            support,
            weight,
        } => {
            sqlx::query!(
                r#"
                INSERT INTO governance_events (tx_id, kind, proposal_id, support, weight)
                VALUES ($1,'vote',$2,$3,$4)
                "#,
                tx_id,
                proposal_id,
                support,
                BigDecimal::from(*weight)
            )
            .execute(&mut **tx)
            .await?;
        }
        TxPayload::PrivacyDeposit { commitment } => {
            sqlx::query!(
                r#"
                INSERT INTO privacy_actions (tx_id, action, commitment)
                VALUES ($1,'deposit',$2)
                "#,
                tx_id,
                commitment.to_vec()
            )
            .execute(&mut **tx)
            .await?;
        }
        TxPayload::PrivacyWithdraw { nullifier, recipient } => {
            sqlx::query!(
                r#"
                INSERT INTO privacy_actions (tx_id, action, nullifier, recipient)
                VALUES ($1,'withdraw',$2,$3)
                "#,
                tx_id,
                nullifier.to_vec(),
                recipient.to_vec()
            )
            .execute(&mut **tx)
            .await?;
            touch_account(tx, recipient, height).await?;
        }
        TxPayload::RollupBridgeDeposit { .. }
        | TxPayload::RollupBridgeWithdraw { .. }
        | TxPayload::Stake { .. }
        | TxPayload::Unstake { .. }
        | TxPayload::SystemUpgrade { .. }
        | TxPayload::Delegate { .. }
        | TxPayload::Undelegate { .. } => { /* already handled or no-op */ }
    }
    Ok(())
}

async fn touch_account(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    address: &[u8; 32],
    height: i64,
) -> anyhow::Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO accounts (address, first_seen_height, last_seen_height, tx_count, updated_at)
        VALUES ($1,$2,$2,1, now())
        ON CONFLICT (address) DO UPDATE SET
            last_seen_height = GREATEST(accounts.last_seen_height, EXCLUDED.last_seen_height),
            tx_count = accounts.tx_count + 1,
            updated_at = now()
        "#,
        address.to_vec(),
        height
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_domain(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    domain_id: &Uuid,
    risk_params: serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO domains (
            domain_id, kind, security_model, sequencer_binding, bridge_contracts, risk_params, created_at, updated_at
        )
        VALUES ($1,'custom','shared_security',NULL,'[]'::jsonb,$2, now(), now())
        ON CONFLICT (domain_id) DO UPDATE SET
            risk_params = EXCLUDED.risk_params,
            updated_at = now()
        "#,
        domain_id,
        risk_params
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn payload_kind(payload: &TxPayload) -> &'static str {
    match payload {
        TxPayload::Transfer { .. } => "transfer",
        TxPayload::Stake { .. } => "stake",
        TxPayload::Unstake { .. } => "unstake",
        TxPayload::Delegate { .. } => "delegate",
        TxPayload::Undelegate { .. } => "undelegate",
        TxPayload::DomainCreate { .. } => "domain_create",
        TxPayload::DomainConfigUpdate { .. } => "domain_config_update",
        TxPayload::RollupBatchCommit { .. } => "rollup_batch_commit",
        TxPayload::RollupBridgeDeposit { .. } => "rollup_bridge_deposit",
        TxPayload::RollupBridgeWithdraw { .. } => "rollup_bridge_withdraw",
        TxPayload::GovernanceProposal { .. } => "governance_proposal",
        TxPayload::GovernanceVote { .. } => "governance_vote",
        TxPayload::PrivacyDeposit { .. } => "privacy_deposit",
        TxPayload::PrivacyWithdraw { .. } => "privacy_withdraw",
        TxPayload::SystemUpgrade { .. } => "system_upgrade",
    }
}

fn payload_events(payload: &TxPayload) -> Vec<String> {
    vec![payload_kind(payload).to_string()]
}

fn tx_hash(tx: &Tx) -> [u8; 32] {
    let bytes = bincode::serialize(tx).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_kind_maps() {
        assert_eq!(
            payload_kind(&TxPayload::Transfer {
                to: [1u8; 32],
                amount: 10
            }),
            "transfer"
        );
        assert_eq!(
            payload_kind(&TxPayload::DomainCreate {
                domain_id: Uuid::nil(),
                params: serde_json::json!({"k": "v"})
            }),
            "domain_create"
        );
    }
}

