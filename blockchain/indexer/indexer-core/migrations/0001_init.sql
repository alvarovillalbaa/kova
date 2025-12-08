-- Core block and transaction storage for explorer/indexer.

CREATE TABLE IF NOT EXISTS blocks (
    height BIGINT PRIMARY KEY,
    hash BYTEA NOT NULL,
    parent_hash BYTEA NOT NULL,
    timestamp_ms BIGINT NOT NULL,
    proposer BYTEA NOT NULL,
    state_root BYTEA NOT NULL,
    l1_tx_root BYTEA NOT NULL,
    da_root BYTEA NOT NULL,
    domain_roots JSONB NOT NULL,
    gas_used BIGINT NOT NULL,
    gas_limit BIGINT NOT NULL,
    base_fee NUMERIC(39, 0) NOT NULL,
    tx_count INT NOT NULL,
    da_blobs JSONB NOT NULL,
    consensus_metadata JSONB NOT NULL,
    inserted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_blocks_timestamp ON blocks (timestamp_ms DESC);

CREATE TABLE IF NOT EXISTS transactions (
    id BIGSERIAL PRIMARY KEY,
    tx_hash BYTEA UNIQUE NOT NULL,
    block_height BIGINT NOT NULL REFERENCES blocks (height) ON DELETE CASCADE,
    position INT NOT NULL,
    chain_id TEXT NOT NULL,
    sender BYTEA NOT NULL,
    nonce BIGINT NOT NULL,
    gas_limit BIGINT NOT NULL,
    gas_price NUMERIC(39, 0),
    max_fee NUMERIC(39, 0),
    max_priority_fee NUMERIC(39, 0),
    payload_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    signature BYTEA NOT NULL,
    success BOOLEAN NOT NULL DEFAULT TRUE,
    events TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_transactions_block_height ON transactions (block_height DESC, position ASC);
CREATE INDEX IF NOT EXISTS idx_transactions_sender ON transactions (sender);

CREATE TABLE IF NOT EXISTS rollup_batches (
    id BIGSERIAL PRIMARY KEY,
    domain_id UUID NOT NULL,
    blob_id TEXT NOT NULL,
    block_height BIGINT NOT NULL REFERENCES blocks (height) ON DELETE CASCADE,
    tx_id BIGINT NOT NULL REFERENCES transactions (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_rollup_batches_domain ON rollup_batches (domain_id);

CREATE TABLE IF NOT EXISTS domains (
    domain_id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    security_model TEXT NOT NULL,
    sequencer_binding UUID,
    bridge_contracts JSONB NOT NULL,
    risk_params JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS governance_events (
    id BIGSERIAL PRIMARY KEY,
    tx_id BIGINT NOT NULL REFERENCES transactions (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    proposal_id UUID,
    support BOOLEAN,
    weight NUMERIC(39, 0),
    payload JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_governance_proposal ON governance_events (proposal_id);

CREATE TABLE IF NOT EXISTS privacy_actions (
    id BIGSERIAL PRIMARY KEY,
    tx_id BIGINT NOT NULL REFERENCES transactions (id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    commitment BYTEA,
    nullifier BYTEA,
    recipient BYTEA,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS accounts (
    address BYTEA PRIMARY KEY,
    first_seen_height BIGINT NOT NULL,
    last_seen_height BIGINT NOT NULL,
    tx_count BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_accounts_activity ON accounts (last_seen_height DESC);
