# Kova L1 Monorepo (Scaffold)

This repository contains scaffolds for the L1 protocol, domains, sequencer, indexer, SDKs, frontend, and ops as defined in `docs/init.md` and the implementation blueprint.

## Layout
- `protocol/` – core Rust crates (consensus, state, DA, VM, runtime, networking, node).
- `domains/` – domain templates (EVM, WASM, privacy, payment).
- `zk/` – circuits/prover/verifier placeholders.
- `contracts/` – system contract stubs.
- `sequencer/` – shared sequencer services.
- `mixnet/` – mixnet adapters.
- `indexer/` – ingest + API.
- `sdk/` – Rust/TS/Python SDKs.
- `frontend/apps/web` – Next.js app routes.
- `ops/` – docker-compose, k8s, CI.

## Quickstart
1. Ensure Rust, pnpm, Python 3.11+, Docker are installed.
2. Build Rust workspace:
   ```bash
   cargo build
   ```
3. Run devnet (multi-validator + sequencer + indexer + frontend):
   ```bash
   make devnet
   ```
   - Uses `ops/docker/genesis.json` for the validator set.
   - Nodes expose RPC on `8545` (validator1). Sequencer on `7545`, indexer on `4000`, frontend on `3000`.
   - Set `GENESIS_PATH` / `VALIDATOR_OWNER` in compose if you customize the validator keys.

## Notes
- The code is scaffolded: interfaces and types are present; logic is intentionally minimal.
- Follow the implementation order from `docs/init.md` when adding functionality.

