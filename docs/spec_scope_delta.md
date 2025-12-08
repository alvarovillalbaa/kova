## Spec vs Build Delta (kova)

Blunt read: most of the spec remains aspirational; the repo is scaffold-heavy. Below is what’s missing versus `docs/spec.md`, and any extras that slipped in.

### Not built yet (per spec)
- ZK stack: no circuits/prover/verifier wiring; `blockchain/zk/` is placeholders only.
- Cross-domain/IBC: only stub packet/header types; no relayer, light-client updates, bridge contracts, or force-inclusion paths.
- L1/L2 contract suite: staking/governance/bridge/privacy contracts are empty stubs; no on-chain slashing, timelocks, or upgrade manager.
- Data availability sampling: in-memory blob store only; no erasure coding, sampling verification, or DA light-node flows; block headers don’t carry real DA commitments.
- Shared sequencer hardening: single in-memory sequencer, no slashing, no MEV/ordering rules, no force-include, no multi-sequencer/rotation.
- Privacy layer: no circuits, nullifier/commitment checks, or shielded accounting; runtime “withdraw” just mints +1 without proofs.
- Governance v2: no proposal lifecycle, voting weights, execution/timelock, or multisig bridge to phase-1 control.
- Economic layer: fee burning and splits are constants; no emission/treasury logic, staking rewards, or domain risk caps/insurance.
- Networking/mempool: no p2p, gossip, or fork-choice integration; HotStuff engine runs without networking or signature verification.
- Domains/templates: EVM/WASM/privacy/payment domains are config macros only; no VM integration, fraud/validity proofs, or domain-specific execution.
- SDKs/devex: TS/Rust SDKs are thin helpers; no tx signing/recovery, no cross-domain helpers, no wallet integration; no CLI.
- Testing: minimal unit tests; no property-based tests, consensus sims, DA withholding tests, fuzzing, or cross-domain test harness.
- Ops: devnet docker-compose exists, but no monitoring, alerting, faucet, or public testnet rollout scripts.
- Frontend: pages exist but are static/read-only; no wallet connect, tx submit, bridging, sequencer control, or governance flows.

### Built/underway (aligned to scope)
- Consensus skeleton: HotStuff-like engine with stake-weighted leader, quorum tracking, commit queue (`blockchain/protocol/consensus`).
- Runtime execution: staking/delegation, domain registry, rollup batch commit, governance placeholders, privacy deposit/withdraw, fee split constants (`blockchain/protocol/runtime`).
- DA/Sequencer stubs: in-memory DA provider + sampler; sequencer batches to DA (`blockchain/protocol/da`, `blockchain/sequencer/core`).
- Devnet scaffolding: monorepo layout, docker-compose with validators/DA/sequencer/indexer/frontend/mixnet stub (`blockchain/ops/docker`).
- Indexer + API: Postgres schema/migrations and Fastify API for blocks/tx/domains/stats/privacy (`blockchain/indexer`).
- Frontend shell: Next.js routes for explorer, domains, staking, governance, wallet, dev, sequencer, testnet (`blockchain/frontend/apps/web`).
- SDK stubs: TS/Rust helpers; minimal Python stub (`blockchain/sdk`).

### Built but not in the original spec (scope creep)
- Python SDK stub (`blockchain/sdk/sdk-python`) — spec called out TS/Rust only.
- Mixnet client/gateway crates with executable main (`blockchain/mixnet/*`) even though mixnet was optional/“use when needed”.
- Extra privacy/indexer endpoints (e.g., `/privacy`, `/stats/mixnet`) beyond the minimal explorer callouts in the spec.

### Quick readout
- You’re still at “scaffold devnet”; core protocol, proofs, domains, and ops hardening remain to be written.
- Highest-risk gaps: ZK/DA correctness, cross-domain security, governance/upgrade safety, and networking/force-inclusion for censorship resistance.

