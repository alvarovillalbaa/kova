import Fastify from "fastify";
import { Pool } from "pg";

const fastify = Fastify({ logger: true });
const pool = new Pool({
  connectionString: process.env.DATABASE_URL,
});

const DEFAULT_LIMIT = 20;

fastify.get("/health", async () => ({ ok: true }));

fastify.get("/stats/chain", async () => {
  const client = await pool.connect();
  try {
    const { rows } = await client.query<{
      count: string;
      min_ts: string | null;
      max_ts: string | null;
      max_height: string | null;
    }>(
      `SELECT COUNT(*) as count, MIN(timestamp_ms) as min_ts, MAX(timestamp_ms) as max_ts, MAX(height) as max_height FROM blocks`
    );
    const row = rows[0];
    const count = Number(row.count || 0);
    const span =
      row.min_ts && row.max_ts
        ? Number(row.max_ts) - Number(row.min_ts)
        : undefined;
    const blockTimeMs = count > 1 && span ? Math.max(span / (count - 1), 0) : 0;
    const tps = blockTimeMs > 0 ? (count * 1000) / blockTimeMs : 0;
    return {
      height: row.max_height ? Number(row.max_height) : 0,
      blocks: count,
      block_time_ms: Math.round(blockTimeMs),
      tps: Number(tps.toFixed(2)),
    };
  } finally {
    client.release();
  }
});

fastify.get("/stats/da", async () => {
  const client = await pool.connect();
  try {
    const { rows } = await client.query<{
      avg_blobs: string | null;
    }>(`SELECT AVG(jsonb_array_length(da_blobs)) as avg_blobs FROM blocks`);
    return { blobs_per_block: Number(rows[0].avg_blobs || 0) };
  } finally {
    client.release();
  }
});

fastify.get("/stats/domains", async () => {
  const client = await pool.connect();
  try {
    const { rows } = await client.query(`SELECT * FROM domains ORDER BY updated_at DESC`);
    return { domains: rows.map(mapDomain) };
  } finally {
    client.release();
  }
});

fastify.get("/stats/sequencer", async () => {
  const client = await pool.connect();
  try {
    const { rows } = await client.query<{ head: string | null }>(
      `SELECT MAX(block_height) as head FROM rollup_batches`
    );
    return { liveness: "ok", head: Number(rows[0].head || 0) };
  } finally {
    client.release();
  }
});

fastify.get("/stats/mixnet", async () => ({ enabled: true }));

fastify.get("/blocks", async (req) => {
  const query = req.query as { limit?: string; offset?: string };
  const limit = Number(query.limit || DEFAULT_LIMIT);
  const offset = Number(query.offset || 0);
  const client = await pool.connect();
  try {
    const { rows } = await client.query(
      `SELECT * FROM blocks ORDER BY height DESC LIMIT $1 OFFSET $2`,
      [limit, offset]
    );
    return rows.map(mapBlock);
  } finally {
    client.release();
  }
});

fastify.get("/blocks/:height", async (req, reply) => {
  const height = Number((req.params as { height: string }).height);
  const client = await pool.connect();
  try {
    const { rows } = await client.query(`SELECT * FROM blocks WHERE height = $1`, [
      height,
    ]);
    if (rows.length === 0) {
      return reply.code(404).send({ error: "not found" });
    }
    const block = mapBlock(rows[0]);
    const txs = await client.query(
      `SELECT * FROM transactions WHERE block_height = $1 ORDER BY position ASC`,
      [height]
    );
    return { ...block, transactions: txs.rows.map(mapTx) };
  } finally {
    client.release();
  }
});

fastify.get("/transactions", async (req) => {
  const query = req.query as { limit?: string; offset?: string; sender?: string };
  const limit = Number(query.limit || DEFAULT_LIMIT);
  const offset = Number(query.offset || 0);
  const params: any[] = [limit, offset];
  const filters: string[] = [];
  if (query.sender) {
    filters.push(`sender = decode($3,'hex')`);
    params.push(strip0x(query.sender));
  }
  const where = filters.length ? `WHERE ${filters.join(" AND ")}` : "";
  const sql = `SELECT * FROM transactions ${where} ORDER BY id DESC LIMIT $1 OFFSET $2`;
  const client = await pool.connect();
  try {
    const { rows } = await client.query(sql, params);
    return rows.map(mapTx);
  } finally {
    client.release();
  }
});

fastify.get("/transactions/:hash", async (req, reply) => {
  const hash = strip0x((req.params as { hash: string }).hash);
  const client = await pool.connect();
  try {
    const { rows } = await client.query(
      `SELECT * FROM transactions WHERE tx_hash = decode($1,'hex')`,
      [hash]
    );
    if (rows.length === 0) {
      return reply.code(404).send({ error: "not found" });
    }
    return mapTx(rows[0]);
  } finally {
    client.release();
  }
});

fastify.get("/domains", async () => {
  const client = await pool.connect();
  try {
    const { rows } = await client.query(`SELECT * FROM domains ORDER BY updated_at DESC`);
    return rows.map(mapDomain);
  } finally {
    client.release();
  }
});

fastify.get("/rollup_batches", async (req) => {
  const query = req.query as { limit?: string; offset?: string; domain_id?: string };
  const limit = Number(query.limit || DEFAULT_LIMIT);
  const offset = Number(query.offset || 0);
  const params: any[] = [limit, offset];
  let sql = `SELECT * FROM rollup_batches`;
  if (query.domain_id) {
    sql += ` WHERE domain_id = $3`;
    params.push(query.domain_id);
  }
  sql += ` ORDER BY id DESC LIMIT $1 OFFSET $2`;
  const client = await pool.connect();
  try {
    const { rows } = await client.query(sql, params);
    return rows;
  } finally {
    client.release();
  }
});

fastify.get("/governance", async (req) => {
  const query = req.query as { limit?: string; offset?: string };
  const limit = Number(query.limit || DEFAULT_LIMIT);
  const offset = Number(query.offset || 0);
  const client = await pool.connect();
  try {
    const { rows } = await client.query(
      `SELECT * FROM governance_events ORDER BY id DESC LIMIT $1 OFFSET $2`,
      [limit, offset]
    );
    return rows;
  } finally {
    client.release();
  }
});

fastify.get("/privacy", async (req) => {
  const query = req.query as { limit?: string; offset?: string };
  const limit = Number(query.limit || DEFAULT_LIMIT);
  const offset = Number(query.offset || 0);
  const client = await pool.connect();
  try {
    const { rows } = await client.query(
      `SELECT * FROM privacy_actions ORDER BY id DESC LIMIT $1 OFFSET $2`,
      [limit, offset]
    );
    return rows.map((row) => ({
      ...row,
      commitment: row.commitment ? toHex(row.commitment) : null,
      nullifier: row.nullifier ? toHex(row.nullifier) : null,
      recipient: row.recipient ? toHex(row.recipient) : null,
    }));
  } finally {
    client.release();
  }
});

fastify.get("/accounts", async (req) => {
  const query = req.query as { limit?: string; offset?: string };
  const limit = Number(query.limit || DEFAULT_LIMIT);
  const offset = Number(query.offset || 0);
  const client = await pool.connect();
  try {
    const { rows } = await client.query(
      `SELECT * FROM accounts ORDER BY last_seen_height DESC LIMIT $1 OFFSET $2`,
      [limit, offset]
    );
    return rows.map((row) => ({
      address: toHex(row.address),
      first_seen_height: Number(row.first_seen_height),
      last_seen_height: Number(row.last_seen_height),
      tx_count: Number(row.tx_count),
      updated_at: row.updated_at,
    }));
  } finally {
    client.release();
  }
});

const port = Number(process.env.PORT || 4000);

async function start() {
  try {
    await fastify.listen({ port, host: "0.0.0.0" });
  } catch (err) {
    fastify.log.error(err);
    process.exit(1);
  }
}

start();

function toHex(bytes: Buffer): string {
  return `0x${bytes.toString("hex")}`;
}

function strip0x(input: string): string {
  return input.startsWith("0x") ? input.slice(2) : input;
}

function mapBlock(row: any) {
  return {
    height: Number(row.height),
    hash: toHex(row.hash),
    parent_hash: toHex(row.parent_hash),
    timestamp_ms: Number(row.timestamp_ms),
    proposer: toHex(row.proposer),
    state_root: toHex(row.state_root),
    l1_tx_root: toHex(row.l1_tx_root),
    da_root: toHex(row.da_root),
    domain_roots: row.domain_roots,
    gas_used: Number(row.gas_used),
    gas_limit: Number(row.gas_limit),
    base_fee: row.base_fee,
    tx_count: Number(row.tx_count),
    da_blobs: row.da_blobs,
    consensus_metadata: row.consensus_metadata,
  };
}

function mapTx(row: any) {
  return {
    id: Number(row.id),
    tx_hash: toHex(row.tx_hash),
    block_height: Number(row.block_height),
    position: Number(row.position),
    chain_id: row.chain_id,
    sender: toHex(row.sender),
    nonce: Number(row.nonce),
    gas_limit: Number(row.gas_limit),
    gas_price: row.gas_price,
    max_fee: row.max_fee,
    max_priority_fee: row.max_priority_fee,
    payload_type: row.payload_type,
    payload: row.payload,
    signature: toHex(row.signature),
    events: row.events,
    created_at: row.created_at,
  };
}

function mapDomain(row: any) {
  return {
    domain_id: row.domain_id,
    kind: row.kind,
    security_model: row.security_model,
    sequencer_binding: row.sequencer_binding,
    bridge_contracts: row.bridge_contracts,
    risk_params: row.risk_params,
    updated_at: row.updated_at,
  };
}
