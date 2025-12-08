## Indexer Stack

- Rust ingestor (`indexer-core` bin `indexer`) tails node RPC (`/get_block/:height`) and writes to Postgres using migrations in `indexer-core/migrations`.
- Fastify API (`indexer/api`) reads Postgres and serves explorer endpoints.

### Run locally

```bash
cd blockchain
export DATABASE_URL=postgres://kova:kova@localhost:5432/kova_indexer
export RPC_URL=http://localhost:8545
cargo run -p indexer-core --bin indexer
```

API:

```bash
cd blockchain/indexer/api
npm install
DATABASE_URL=postgres://kova:kova@localhost:5432/kova_indexer npm run dev
```

### Devnet (docker-compose)

- `indexer_ingest` runs Rust ingestor (migrations auto-run).
- `indexer_api` runs Fastify with Postgres URL.

Ensure Postgres is up; if schema changes, rebuild images.
