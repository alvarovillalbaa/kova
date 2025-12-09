# Spec Alignment Review (vs `docs/spec.md`)

## Snapshot
- Stage: scaffold devnet. Single binary node with HotStuff-like toy engine, in-memory DA, stub ZK hooks, domain/rollup placeholders, basic REST. No end-to-end secure chain, no contracts or bridges deployed.
- Fit to spec: broad module layout matches intent; depth is thin. Most safety-critical pieces (consensus networking, slashing, DA sampling robustness, bridges, proofs, governance, ops) are missing or stubbed.

## Area-by-area delta
- Consensus: Stake-weighted HotStuff skeleton with commit queue and prop/vote signatures (`blockchain/protocol/consensus/src/lib.rs`). No persistent storage, fork choice, or real view-change; timeouts just bump view. Slashing only records evidence; no penalties. Networking is libp2p-if-available else noop (`protocol/node/src/main.rs`, `protocol/networking`). No mempool anti-spam or block verification beyond signatures.
- Data Availability: In-memory DA with XOR parity + Merkle commitment, deterministic sampling seed, HTTP proofs (`protocol/da/src/lib.rs`; node routes `/da/*`). No erasure coding audits, no distributed storage, no slashing for withholding, no DA light-client integration.
- Execution / Runtime: Large Tx enum with staking, domains, rollup, governance, privacy, cross-domain calls (`protocol/runtime/src/lib.rs`). State uses in-memory store; gas is flat constants; fee split constants, no burning/treasury accounting. Rewards/inflation structs exist but payouts are simplistic; no epoch processing or slashing enforcement. Signature verification only ed25519; no replay protection beyond nonce.
- Domains & Cross-domain: Domain runtime supports registering EVM/WASM adapters only; EVM adapter is a stub that hashes inputs into KV (`runtime/src/domains/{mod,evm}.rs`). No fraud/validity proofs, no bridge roots, no cross-domain inbox/outbox wiring to L1 header roots.
- Sequencer: In-memory sequencer batching to DA with optional SP1 rollup proof generation; round-robin leader and force-include queue exist but not tied to L1 (`sequencer/core/src/lib.rs`). API/coordinator are minimal; no slashing/MEV rules or force-inclusion contract.
- ZK: SP1 backend optional via env; loads ELF artifacts for block/rollup/privacy proofs (`protocol/node/src/main.rs`, `zk/sp1`). If disabled, privacy uses stub verification; block/rollup proofs are optional and not enforced by consensus.
- Privacy: Deposit/withdraw flow with nullifier set and optional SP1 verify; Merkle is local list hash (`runtime/src/lib.rs`). No real circuit security, no shielding of metadata, no mixers on network path; withdraw succeeds with stub proofs if zk backend off.
- Governance/Upgrades: Tx types and REST for proposals exist; contract handlers are no-op (`contracts/governance`, `protocol/node/src/main.rs`). No proposal lifecycle, voting weights, timelocks, or multisig controls.
- Economic layer: Fee split constants only; no emission schedule, staking rewards distribution, treasury accounting, or domain risk caps (`runtime/src/lib.rs`). Gas pricing is simple base_fee + optional tips; no EIP-1559 adjustments.
- Bridges / Rollups: Contracts (`contracts/rollup_bridge`, `domains_*`) are empty stubs; RollupBatchCommit just records blob_id in runtime, without L1 anchoring or exit/entry correctness. No force-inclusion or fraud/validity gates on batches.
- Networking / RPC: Axum REST only: health, status, governance view, privacy pool, send_raw_tx, block fetch, DA endpoints (`protocol/node/src/main.rs`). No WebSocket, gRPC, or p2p tx/block gossip beyond consensus messages; mempool is local Vec with limit.
- SDKs / DevEx: Rust/TS/Python SDK crates exist but are thin; CLI stub; no signing helpers, wallet integration, or cross-domain utils (`sdk/*`). No test vectors.
- Indexer / Explorer: Postgres schema + Fastify API with stats and CRUD endpoints (`indexer/api/src/server.ts`, `indexer/indexer-core`). Depends on data ingestion that is not wired from node. Frontend Next.js routes are static placeholders (`frontend/apps/web/app/*`).
- Ops / Devnet: docker-compose for validators/DA/sequencer/indexer/frontend/mixnet exists (`ops/docker`). No monitoring wiring (Prometheus config present but not hooked), no faucet/testnet orchestration beyond scripts, no CI coverage.
- Mixnet: Client/gateway crates with executable stub, not integrated into node RPC or wallets (`mixnet/*`). Optional per spec, currently unused.

## What’s built (aligned with spec)
- Module layout mirrors the blueprint: protocol/consensus, DA, runtime, domains, sequencer, zk, contracts, sdk, indexer, frontend, ops.
- HotStuff-like engine with stake-weighted leader and commit queue; property/smoke tests exist (`protocol/consensus/tests`).
- In-memory DA with commitments and sampling proof verification; HTTP endpoints exposed.
- Runtime covers staking/delegation, domain registry hooks, rollup batch commit, governance structs, privacy pool logic, fee split constants.
- Sequencer batches to DA and can generate/verify SP1 rollup proofs when enabled.
- SP1 artifacts present for block/rollup/privacy; zk backend pluggable.
- Indexer schema + API for blocks/tx/domains/stats; docker-compose wires components together.
- Frontend routes for explorer/domains/staking/governance/wallet/dev/sequencer/testnet exist (though static).

## Gaps vs spec (highest-risk first)
- Safety: No real consensus networking, fork choice, slashing, persistence, or validator set updates; nodes can disagree silently. Privacy and rollup proofs are optional/stubbed; DA sampling is trusted local memory.
- Bridging/cross-domain: No light clients, no bridge contracts, no force-inclusion, no cross-domain verification. Domain adapters are mock-only.
- Governance/upgrades: No proposal lifecycle or control plane; upgrades unchecked.
- Economics: No issuance/rewards/treasury flows; fee burn/splits are constants only.
- Ops/test: No property/fuzz for runtime/DA/bridges/privacy; no chaos/partition sims; no monitoring/alerting; devnet lacks faucets/explorer data.
- DevEx: SDKs lack signing and network coverage; frontend not wired to APIs; no wallet connect.

## Scope creep (not in spec)
- Python SDK stub (`sdk/sdk-python`).
- Mixnet crates with bins (`mixnet/*`) even though mixnet was optional.
- Extra indexer endpoints (privacy/mixnet stats) beyond minimal explorer scope.

## Suggested next steps
- Hardening path: add persistent storage + verified block application, real view-change, validator set updates and slashing enforcement; wire libp2p gossip for blocks/tx; add fork-choice and WAL.
- Security/DA: replace in-memory DA with networked storage and real erasure coding; random sampling, light-node verification, and slashing for withholding.
- Bridges/domains: implement bridge contracts + light-client/validity proofs; real domain adapters (EVM/WASM), cross-domain inbox/outbox roots in headers, and force-inclusion from sequencer to L1.
- ZK: enforce proof verification on block/rollup/privacy paths; remove stub bypass; add test vectors.
- Governance/economics: implement proposal/vote/timelock, emission + rewards distribution, fee burning/treasury accounting, staking withdrawals with slashing.
- DevEx/ops: wire SDK signing + RPCs, connect frontend to APIs, add faucet and explorer data, integrate monitoring/alerting, expand tests (property/fuzz/partition) and CI.

## Verdict
You’re at “demo devnet”: architecture matches the ambition, but correctness, security, and operability layers from the spec are largely unbuilt. Focus on consensus/DA/bridge correctness and governance/ops before adding more surface area.
