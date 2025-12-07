# Implementation Blueprint (Build-Ready Extract)

## 1) Scope, Assumptions, Priorities
- Goal: Stand up an L1 with HotStuff-style PoS, integrated DA sampling, zk-VM ready hooks, domains (rollups/appchains), shared sequencer, privacy pools, single token X, and full DevEx (SDKs, explorer, wallet).
- Language/runtime: Rust protocol; Next.js (TS) frontend; Postgres indexer; Docker/ECS target.
- Phasing: Devnet → internal testnet → public testnet → mainnet-beta.
- Constraints: Core consensus + DA are fixed; modules are narrowly scoped; avoid N+1 in storage; favor typed interfaces and bounded async.
- Non-goals: PoW, fully pluggable kernel; keep kernel small, extend via modules/precompiles.

## 2) Module Contracts & Interfaces

### 2.1 State Model (L1)
- Global sparse Merkle tree.
- Entities:
  - `Account { address, nonce, balance_x, code_hash?, storage_root? }`
  - `Validator { id, pubkey, stake, status, commission_rate }`
  - `Delegation { delegator, validator_id, stake }`
  - `DomainEntry { domain_id, type, security_model, sequencer_binding, bridge_contracts, risk_params }`
  - `DACommitment { block_height, da_root, blob_ids[] }`
  - `DomainRoot { domain_id, state_root, da_root, last_verified_epoch, proof_meta }`
  - `Proposal { id, payload, kind, status, votes, timers }`
  - Fee pools { l1_gas, da, sequencer, treasury }
  - `PrivacyPool { merkle_root, parameters, nullifiers }`

### 2.2 Block & TX
- `BlockHeader { parent_hash, height, timestamp, proposer_id, state_root, l1_tx_root, da_root, domain_roots[], gas_used, gas_limit, base_fee, consensus_metadata(QC) }`
- `Tx { chain_id, nonce, gas_limit, gas_price | (max_fee,max_priority_fee), payload, signature }`
- Tx payload types: transfer, stake/unstake/delegate, domain_create/config, rollup_batch_commit, rollup_bridge_{deposit,withdraw}, governance_{proposal,vote}, privacy_{deposit,withdraw}, system_upgrade.

### 2.3 Consensus (HotStuff-like)
- Roles: validators (stake-weighted), tolerate f = floor((n-1)/3).
- States: view, height, locked QC, pending QC.
- Phases per block: PREPARE → PRECOMMIT → COMMIT with chained QCs (3-chain finality).
- Leader rotation: stake-weighted round-robin; view-change on timeout.
- Slashing: double-sign, invalid block, DA fraud.
- Interfaces:
  - `ConsensusEngine::propose(block)` (leader)
  - `ConsensusEngine::vote(block_id, view)`
  - `ConsensusEngine::on_qc(qc)` → update locked block
  - `TimeoutManager::on_timeout(view)` → trigger view change

### 2.4 Data Availability + DAS
- DA matrix commitment per block; `da_root` in header.
- Roles: full validators store blobs; DA light nodes sample.
- Interfaces:
  - `SubmitBlob(domain_id, blob_bytes) -> BlobId`
  - `GetBlob(BlobId) -> bytes`
  - `ProveBlobAvailability(BlobId) -> DAProof`
- Rewards: DA fee pool split to validators/light nodes.

### 2.5 Execution / zk-VM Hooks
- Kernel exposes syscalls: `read_state`, `write_state`, `emit_event`, `call_precompile`, `verify_zk_proof`.
- Precompiles: hashes (Poseidon, Keccak, SHA-2), curves (BLS12-381, Pasta), signatures (ed25519, secp256k1), ZK helpers (MSM, FFT), privacy primitives (commitments, Merkle).
- System contracts (Rust/WASM): staking, governance, domains registry, bridges, privacy pools.

### 2.6 Domains & Rollups
- Domain shape: `{ id, type(EVM|WASM|PRIVACY|PAYMENT|CUSTOM), security_model(shared|own), execution_vm, da_mode(oncain|offchain|volition), sequencer_binding(shared|dedicated), token_model(X|local), risk_params }`
- Templates: EVM shared security, ZK privacy domain, sovereign appchain, payment-channel domain.
- Shared sequencer:
  - API: POST `/v1/submit_tx {domain_id, tx_bytes, fees, nonce}`; GET `/v1/domain_head`; GET `/v1/batch_status`.
  - Force-inclusion path via L1 contract.
  - Slashing: invalid batches, malformed proofs, provable censoring (if measurable).

### 2.7 Cross-Domain Messaging (IBC-ish)
- Packet: `{ src_domain, dst_domain, sequence, payload, timeout_height }`
- Light-client baseline: headers + validator set changes + state roots.
- ZK-enhanced: proofs of state transition and packet inclusion.
- Internal vs external: internal cheaper proofs; external uses light clients/bridges.

### 2.8 Privacy Layer
- Shielded pool circuits: deposit (commitment), withdraw (nullifier + Merkle inclusion + range proof), notes/nullifiers/stealth addresses.
- Mixnet adapters: optional RPC over mixnet; flag `--use-mixnet` for node/CLI.

### 2.9 Governance & Upgrades
- Phase 1: council/multisig + signaling; scope = params, modules, domains, upgrades.
- Phase 2: on-chain proposals/votes, timelocks, emergency veto.
- Contracts:
  - `GovernanceModule::{submit_proposal, vote, queue_execution, execute_proposal}`
  - `UpgradeManager::{load_module, apply_migration}`

### 2.10 Node RPC (JSON-RPC/gRPC/WS)
- Chain data: `get_block`, `get_tx`, `get_state`.
- Accounts: `get_balance`, `get_nonce`.
- Staking: `get_validators`, `get_delegations`.
- Domains: `list_domains`, `get_domain`.
- Rollups: `get_domain_head`, `get_rollup_batch`.
- Governance: `get_proposals`, `get_votes`.
- Tx submission: `send_raw_tx`.
- WS subs: `subscribe_new_blocks`, `subscribe_events`.

### 2.11 SDKs
- Rust protocol SDK: `StateAccess` trait (`get,set,iterate`); `Module` trait (`init, handle_tx, handle_block_begin/end`); crypto helpers.
- Domain template SDK: `define_domain!{...}` macro; genesis generator; sequencer binding; registry registration.
- TS dApp SDK (`@xchain/sdk`): key mgmt, tx build/sign, queries (RPC/indexer), bridge helpers, cross-domain messaging, privacy ops.

### 2.12 Indexer & Explorer API
- Ingest blocks/tx/events/batches → Postgres.
- GraphQL/REST: `blocks`, `transactions`, `accounts`, `domains`, `governance`, `rollup_batches`, `privacy_pool_stats`.
- Aggregated endpoints: `/stats/chain|da|domains|sequencer|mixnet`.

### 2.13 Frontend (Next.js App Router)
- Routes: `/` (overview), `/explorer`, `/domains`, `/governance`, `/staking`, `/wallet` (incl. bridge + privacy), `/dev`, `/sequencer`, `/testnet`.
- Tech: Next.js (TS), Tailwind, shadcn/ui, React Query, `@xchain/sdk`.
- Multi-endpoint support: node RPC, indexer, sequencer; network switcher (devnet/testnet/mainnet); fallback UIs for DA down / domain paused.

## 3) Environment Scaffolding (Devnet)
- Repo layout (monorepo):
  - `protocol/` (consensus, state, da, vm, runtime, networking, node)
  - `domains/` (evm_domain, wasm_domain, privacy_domain, payment_domain)
  - `zk/` (circuits, prover, verifier)
  - `contracts/` (l1, rollup_bridge, governance, staking, domains_registry, privacy_pools)
  - `sequencer/` (core, api, coordinator)
  - `mixnet/` (client, gateway)
  - `indexer/` (indexer-core, api)
  - `sdk/` (sdk-rust, sdk-ts, sdk-python)
  - `frontend/` (apps/web)
  - `ops/` (docker/, k8s/, ci/)
  - `docs/`
- Docker-compose devnet:
  - 4 validator nodes (protocol) + 1 DA light node.
  - 1 sequencer (shared) posting batches to DA.
  - 1 indexer + API.
  - Postgres for indexer.
  - Frontend web app.
  - Optional mixnet stub service.
- Config:
  - `genesis.json` with chain_id, validators, supply, params (block time, max gas, DA sample count, slashing fractions).
  - Fee splits: `{ l1_gas: burn 30%, validators 70%; da_fees: validators 70%, da_nodes 20%, treasury 10%; l2_fees: sequencer 50%, da_costs 30%, l1_rent 20% }`
  - Ports: Node RPC 8545/26657-like, gRPC 9090, WS 8546, sequencer API 7545, indexer API 4000, frontend 3000.

## 4) Phased Delivery, Risks, Testing
- Order (from spec):
  1) Core libs (crypto, storage).
  2) Consensus skeleton + basic `TRANSFER_X`.
  3) Node & RPC (single-node).
  4) DA blobs + commitments + sampling.
  5) Full PoS staking.
  6) zkVM integration + system contracts.
  7) Domains v1 (EVM shared-security) + bridge.
  8) Shared sequencer v1.
  9) Cross-domain messaging v1.
  10) Privacy pools v1.
  11) Indexer + Explorer.
  12) Public Testnet.
  13) Additional domains (WASM, privacy, payment).
  14) Governance/Upgrades v2.
  15) Mainnet Beta.
- Testing layers: unit (state, consensus, crypto), property-based (invariants), consensus sims (partitions/faulty leaders), DA sampling tests (withholding), fuzzing (RPC/tx/VM), differential for EVM domains, integration for bridges and privacy circuits.
- Risks & mitigations:
  - Consensus safety regressions → property-based + chained-QC invariants.
  - DA withholding → light-node sampling tests; slash on invalid DA.
  - Bridge/cross-domain correctness → light-client proofs + fraud/validity proofs; caps/risk_params on domains.
  - Sequencer censorship → force-inclusion path; monitor liveness.
  - Privacy circuit bugs → external audits + test vectors.
  - Performance → pipelined HotStuff, bounded channels, metrics; offload heavy crypto to precompiles.

---

## Scaffold status (devnet)
- Monorepo directories and Cargo/pnpm workspaces in place.
- Devnet docker-compose wired for validators, DA light, sequencer, indexer, Postgres, frontend, mixnet stub.
- Protocol stubs: HotStuff engine, in-memory state store, DA provider/sampler, RPC node with basic block production, transfer execution, fee split constants.
- System stubs: staking/governance/domain registry/bridge/privacy contract hooks; sequencer API and batch builder; domain templates with `define_domain!` macro for EVM.
- DevEx stubs: TS/Rust SDK helpers, indexer API stats endpoints, Next.js routes for explorer, domains, governance, staking, wallet, dev, sequencer, testnet.

