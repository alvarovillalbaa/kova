Here’s a full, implementation-oriented spec you can hand to another AI as “build this whole thing”. I’ll assume:

* **Protocol language:** Rust
* **Frontend:** Next.js (TypeScript, App Router)
* **Primary model:** PoS + BFT HotStuff-like L1 with integrated DA sampling, zk-VM, domains (rollups/appchains), shared sequencer, privacy, single native token, and plugin-like modularity.

---

# 0. Goals & Non-Goals

**Goals**

1. New L1 with:

   * PoS + BFT **HotStuff-style** consensus. ([arXiv][1])
   * Integrated **data availability sampling** (DAS) for scalable DA. ([Celestia][2])
   * **zk-VM / zk-EVM** execution with STARK-based proofs.
   * **Domains** abstraction: rollups, subnets, appchains, L3s as parameterized instances.
   * **Shared sequencer** for multiple rollups/domains. ([The Flashbots Collective][3])
   * **Privacy layer:** shielded pools + optional **mixnet** integration (Nym-like). ([Nym][4])
   * **Single native token X** for L1, DA, sequencer, mixnet, governance.

2. From day-one:

   * **Devnet → public testnet → mainnet-beta** pipeline.
   * **Full SDKs**, explorer, wallet, governance UI, domain manager, docs.

**Non-Goals**

* No PoW.
* No “everything is pluggable” kernel: **core consensus + DA rules are fixed**, only exposed via carefully scoped modules.

---

# 1. Monorepo & High-Level Structure

Use a single monorepo (e.g. `blockchain/`) with:

```txt
blockchain/
  protocol/           # Rust crates: consensus, state, DA, VM, node binary
    consensus/
    state/
    da/
    vm/
    runtime/
    networking/
    node/
  domains/            # Domain templates (EVM, WASM, private, payment-channel)
    evm_domain/
    wasm_domain/
    privacy_domain/
    payment_domain/
  zk/                 # Circuits + prover/verifier integration (STARKs, zkVM)
    circuits/
    prover/
    verifier/
  contracts/          # On-chain system contracts (Rust/WASM or Solidity if EVM-domain)
    l1/
    rollup_bridge/
    governance/
    staking/
    domains_registry/
    privacy_pools/
  sequencer/          # Shared sequencer service
    core/
    api/
    coordinator/
  mixnet/             # Integration/adapters to external mixnet
    client/
    gateway/
  indexer/            # Off-chain indexer + GraphQL/REST API
    indexer-core/
    api/
  sdk/                # SDKs
    sdk-rust/
    sdk-ts/
    sdk-python/
  frontend/           # Next.js app(s)
    apps/web/         # Explorer+wallet+governance+dev tools
  ops/                # DevOps, infra, docker compose, k8s manifests
    docker/
    k8s/
    ci/
  docs/               # Markdown specs, docs site content
```

---

# 2. Core L1 Protocol Spec

## 2.1 State Model (L1)

Define a **single global state tree** (e.g. Merkle-Patricia or Sparse Merkle tree):

* **Accounts**

  * `Address` (public key hash).
  * `nonce`.
  * `balance_X`.
  * Optional `code_hash` + `storage_root` (if you allow native contracts on L1).
* **Staking / Validators**

  * `ValidatorSet`:

    * `validator_id`, `pubkey`, `stake`, `status {active, jailed, exited}`, `commission_rate`.
  * Delegations (if you allow delegators).
* **Domains Registry**

  * `DomainId` →

    * `type`: `EVM_SHARED_SECURITY`, `SOVEREIGN`, `PAYMENT_DOMAIN`, etc.
    * `security_model`: `SHARED_SECURITY`, `OWN_SECURITY`.
    * `sequencer_binding`: id(s) of sequencer(s) serving this domain.
    * `bridge_contracts`: addresses on L1 & domain.
    * `risk_params`: caps, slippage, limits.
* **DA Commitments**

  * Per block: commitments to DA blobs (polynomial commitments / Merkle roots).
* **Rollup / Domain Roots**

  * For each `DomainId`:

    * Latest `state_root`, `da_root`, `last_verified_epoch`, proof metadata.
* **Governance**

  * `Proposal` objects, votes, timers.
* **Fee Pools**

  * `fee_pool_l1`, `fee_pool_da`, `fee_pool_sequencer`, `treasury`.
* **Privacy Pools**

  * Merkle trees for shielded notes.
  * Parameters for circuits.

## 2.2 Block Structure

Define:

```txt
Block {
  header: {
    parent_hash,
    height,
    timestamp,
    proposer_id,
    state_root,
    l1_tx_root,
    da_root,              # Commitment to DA blobs
    domain_roots[],       # Per-domain commitment (optional)
    gas_used,
    gas_limit,
    base_fee,
    consensus_metadata,   # QC, signatures etc (HotStuff)
  },
  transactions: [L1Transaction],
  da_blobs: [BlobRef],    # Actual data may live off-header, referenced here
}
```

* DA blobs are committed in `da_root` and stored in DA layer with sampling.

## 2.3 Transaction Types

Base L1 tx types:

* `TRANSFER_X`
* `STAKE`, `UNSTAKE`, `DELEGATE`, `UNDELEGATE`, `WITHDRAW_REWARDS`
* `DOMAIN_CREATE`, `DOMAIN_CONFIG_UPDATE`
* `ROLLUP_BATCH_COMMIT` (submit rollup batch + proof)
* `ROLLUP_BRIDGE_DEPOSIT`, `ROLLUP_BRIDGE_WITHDRAW`
* `GOVERNANCE_PROPOSAL`, `GOVERNANCE_VOTE`
* `PRIVACY_DEPOSIT`, `PRIVACY_WITHDRAW` (shielded pools)
* `SYSTEM_UPGRADE` (trigger module version changes)

Each tx has:

```txt
Tx {
  chain_id,
  nonce,
  gas_limit,
  gas_price (or EIP-1559 style: max_fee, max_priority_fee),
  payload: TxPayload,
  signature,
}
```

## 2.4 Gas & Fees

* Adopt **EIP-1559 style** pricing:

  * `base_fee` per gas on L1, partially **burned** and partially given to validators.
* Gas dimensions:

  * compute, storage, DA bytes, zk verification cost.
* Map fees to pools:

  * L1 gas → validators + burn.
  * DA bytes → validators + DA light nodes + treasury.
  * Rollup/domain fees → sequencer share + DA costs + security rent to L1 (see §8).

## 2.5 Consensus: PoS + BFT (HotStuff-like)

Implement HotStuff-style protocol: leader-based, partially synchronous, linear communication. ([arXiv][1])

* **Replicas:** validators with stake ≥ min_threshold.
* **Safety:** tolerate up to `f = floor((n-1)/3)` Byzantine validators.
* **Phases per block:**

  * `PREPARE` → `PRECOMMIT` → `COMMIT` with **Quorum Certificates (QC)**.
  * Pipelined for throughput.
* **Fork choice:** follow highest QC.
* **Leader rotation:**

  * round-robin weighted by stake with view-change on timeout.
* **Finality:**

  * Block finalized once 2 chained QCs confirm it (3-chain property).
* **Validator lifecycle:**

  * Join via `STAKE` tx; active after warm-up.
  * Slashing for:

    * double signing,
    * invalid block proposals,
    * participation in invalid DA (block with unavailable data).

Formalize:

* State machine for consensus (view, height, locked QC, etc.).
* Timeouts and network assumptions.

## 2.6 Genesis & Config

Define a `genesis.json`:

* `chain_id`
* `initial_validators` (pubkey, stake, address)
* `initial_supply` of X:

  * `founders`, `community`, `treasury`, `airdrops`.
* Initial parameters:

  * block time target,
  * max gas per block,
  * DA sampling parameters (sample count, confidence threshold),
  * slashing fractions.

---

# 3. Data Availability Layer (DA + DAS)

Implement an **L1-integrated DA layer** with **data availability sampling** similar in spirit to Celestia. ([Celestia][2])

## 3.1 Data Model

* For each block:

  * Build a 2D Reed-Solomon encoded data matrix (or similar).
  * Commit to the root(s) in `da_root` in the header.
* **Light DA nodes**:

  * Sample random chunks of the matrix.
  * If enough independent nodes can retrieve samples, they consider data available.

## 3.2 DA Roles

* **Full validators**:

  * Store full block & DA blobs for at least `N` epochs.
* **DA light nodes**:

  * Only store samples.
  * Participate in DA sampling and gossip.
* Reward DA participants from **DA fee pool**.

## 3.3 DA API for Rollups/Domains

Expose an internal protocol interface:

* `SubmitBlob(domain_id, blob_bytes) -> BlobId`

  * Charged per byte in X.
* `GetBlob(BlobId) -> blob_bytes`
* `ProveBlobAvailability(BlobId) -> DAProof`

  * Used by rollups and external verifiers.

---

# 4. Execution Layer: zk-VM / zk-EVM

Core requirements:

* **General-purpose VM** that can be proven with STARK-style proofs.
* Keep **kernel logic non-programmable by users**; user-level programmability mostly via **domains** (EVM/WASM).

## 4.1 VM Spec

* Instruction set:

  * Option 1: RISC-style zkVM (RISC-V-like).
  * Option 2: zkEVM opcode set (for EVM-compatibility).
* System calls:

  * `read_state(key)`
  * `write_state(key, value)`
  * `emit_event(topic, data)`
  * `call_precompile(id, input)`
  * `verify_zk_proof(proof, vk)`

## 4.2 System Modules & Precompiles (Plugin Level 1)

System modules expose advanced crypto as **precompiles**, not as kernel changes:

* Hashes (Poseidon, Keccak, SHA-2).
* Elliptic curves for ZK (BLS12-381, Pasta, etc).
* Signature schemes (ED25519, secp256k1).
* ZK helpers (multi-scalar multiplication, FFT).
* Privacy primitives:

  * Pedersen commitments,
  * Merkle trees (Poseidon/Rescue).

Each module:

* Has `name`, `version`, `capabilities` (what state it can read/write).
* Activated via governance.

## 4.3 Execution Model

* For L1:

  * Restricted VM usage for **system contracts**: staking, governance, domains registry, bridges, privacy pools.
* For domains:

  * VM instanced per domain (EVM or WASM).

---

# 5. Domains & Rollups Architecture

Adopt **“domain”** as the unified abstraction:

```txt
Domain {
  id: DomainId,
  type: EVM | WASM | PRIVACY | PAYMENT | CUSTOM,
  security_model: SHARED_SECURITY | OWN_SECURITY,
  execution_vm: EVM | WASM | custom zkVM,
  da_mode: ONCHAIN_DA | OFFCHAIN_DA | VOLITION,
  sequencer_binding: shared | dedicated,
  token_model: uses X | has local token (bridged to X),
  risk_params: caps, collateral requirements, etc.
}
```

## 5.1 Domain Types

Initial templates:

1. **EVM_SHARED_SECURITY_DOMAIN**

   * EVM execution.
   * Security via L1:

     * rollup contracts on L1 verify state roots / proofs.
   * Fees in X (local token optional but settles to X).
2. **ZK_PRIVACY_DOMAIN**

   * Restricted VM, circuits for private DeFi/identity.
   * Tighter privacy rules.
3. **SOVEREIGN_APPCHAIN_DOMAIN**

   * Own validator set and consensus.
   * Uses L1 only for DA and bridging.
4. **PAYMENT_CHANNEL_DOMAIN**

   * Off-chain channels with periodic settlement on L1.
   * Optimized for B2B periodic settlement.

Domains are created/configured via `DOMAIN_CREATE` txs and a **Domain Registry** contract on L1.

## 5.2 Shared Sequencer

A **shared sequencer network** serves multiple domains/rollups, improving UX and atomic multi-domain transactions. ([The Flashbots Collective][3])

Sequencer responsibilities:

* Accept txs from users for any `DomainId`.
* Order txs and build **L2 blocks** per domain.
* Post **batches** (with DA blobs) to L1 DA layer.
* Optionally generate proofs (if integrated with prover).

Sequencer API:

```http
POST /v1/submit_tx
  body: { domain_id, tx_bytes, fees, nonce }

GET  /v1/domain_head
  params: { domain_id }

GET  /v1/batch_status
  params: { domain_id, batch_id }
```

Security:

* Sequencer set is PoS-secured with X.
* Misbehavior:

  * Censorship → users can do **force inclusion** directly via L1 contract.
  * Faulty ordering w/ invalid batches → L1 rejects proofs & slashes sequencer.

## 5.3 Cross-Domain Messaging (IBC-like)

Design an **IBC-style protocol** for cross-domain messaging with optional ZK proofs:

### 5.3.1 Light-Client Baseline

* Each domain maintains a **light client** of its counterparty:

  * headers, validator set changes, state roots.
* Messages:

  * `Packet { src_domain, dst_domain, sequence, payload, timeout_height }`.
* Relayers:

  * Off-chain processes that transport packets between domains.
  * Permissionless; security comes from verification on-chain.

### 5.3.2 ZK-Enhanced Variant

* Replace some light-client verifications with ZK proofs:

  * Validity proofs for state transitions in a domain.
  * Validity of packet inclusion without replay.

### 5.3.3 Internal vs External

* **Internal cross-domain**:

  * For domains living in same L1 ecosystem; may use cheaper proofs.
* **External**:

  * To/from Ethereum, Cosmos, etc:

    * Light-client contracts on both sides (plus optional ZK).
    * Bridges for tokens & messages.

---

# 6. Privacy Layer

## 6.1 On-Chain Privacy (Shielded Pools)

Implement generic **shielded pool** contracts:

* ZK circuits for:

  * deposit: `public (commitment), private (amount, recipient)`.
  * withdraw: prove ownership & correct balances using Merkle tree inclusion + range proofs.
* Primitives:

  * stealth addresses,
  * note commitments,
  * nullifiers.

Design for:

* Fungible token privacy (X and ERC-20-like assets).
* Optionally, private stateful dApps in privacy domains.

## 6.2 Network Privacy (Mixnet)

Integrate with a **Nym-like mixnet**:

* Standalone mixnet network (out of scope to re-implement fully, but provide adapters). ([Nym][4])
* Provide:

  * `mixnet-client` library for wallets / nodes:

    * wrap RPC calls through mixnet,
    * hide IP and timing patterns.
  * Config flags:

    * `--use-mixnet` for node and CLI.
* UX:

  * Default: normal TLS RPC.
  * Optional: privacy-enhanced RPC over mixnet for high-sensitivity cases.

---

# 7. Economic & Token Model (X)

## 7.1 Native Token X

Attributes:

* Fixed or capped supply with optional tail emission, or inflationary but bounded.
* Used for:

  * L1 gas.
  * DA fees.
  * Sequencer staking & rewards.
  * Validator staking & rewards.
  * Mixnet fees.
  * Governance voting power (possibly combined with delegated stake).

## 7.2 Fee Flows

Implement the earlier **actor → revenue → incentives** table in code:

* L1 validators:

  * Receive share of:

    * L1 gas (minus burn),
    * DA fees,
    * inflation.
* DA light nodes:

  * Receive portion of DA fees proportional to sampling participation.
* Sequencers:

  * L2 gas share (paid by users/domains) + potential MEV (if allowed).
* Domain operators:

  * Receive `security_rent` or share of local fees.
* Mixnet nodes:

  * Paid in X per traffic volume + quality score.

Implement fee routing via protocol constants in governance-controlled module:

```txt
fee_split {
  l1_gas: { burn: 30%, validators: 70% },
  da_fees: { validators: 70%, da_nodes: 20%, treasury: 10% },
  l2_fees: { sequencer: 50%, da_costs: 30%, l1_rent: 20% },
}
```

---

# 8. Security Model & Slashing

Use the layered security matrix you outlined and encode it explicitly in spec:

1. **L1 PoS+BFT:** safety & liveness for base chain.
2. **DA layer:** DA sampling rules (blocks w/o DA → invalid).
3. **Shared sequencer:** ordering & censorship-resistance, but not finality.
4. **Domains:**

   * `shared-security` → L1 validates.
   * `own-security` → trust boundaries & bridge risk limits.
5. **Mixnet:** metadata privacy only; does not touch consensus.

For each layer:

* Define slashing conditions:

  * L1: double sign, invalid blocks, DA fraud.
  * Sequencers: invalid batches, inclusion of malformed proofs, provable censoring (if measurable).
  * Mixnet (if staked): misreporting, downtime.
* Define **“escape hatches”**:

  * Force inclusion: send tx with higher fee directly to L1 rollup contract.
  * Domain pause: L1 governance can pause unsafe domain.

---

# 9. Governance & Upgrades

Two-phase governance:

1. **Phase 1 – Early Mainnet**

   * Multi-sig / council + token signaling.
   * Governance scope:

     * parameter changes (gas, DA config),
     * module activation,
     * domain templates,
     * contract upgrades.
2. **Phase 2 – Decentralized Governance**

   * On-chain proposals & voting:

     * staking-weighted, optionally with quadratic overlays.
   * Timelocks for upgrades.
   * Emergency veto rules (time-limited).

## 9.1 Governance Contracts

Implement:

* `GovernanceModule`:

  * `submit_proposal(payload, kind)`
  * `vote(proposal_id, support, weight)`
  * `queue_execution(proposal_id)`
  * `execute_proposal(proposal_id)`
* `UpgradeManager`:

  * Loads new wasm modules / precompiles.
  * Handles migrations (state migrations on major version changes).

---

# 10. Node Software & External APIs

## 10.1 Node Binary

`node` binary responsibilities:

* Consensus engine (HotStuff).
* P2P networking (gossip, block propagation).
* DA sampling client.
* State execution (apply blocks).
* RPC services.

## 10.2 RPC API

Expose **JSON-RPC + gRPC + WebSocket**:

Core calls:

* Chain data:

  * `get_block(height|hash)`
  * `get_tx(hash)`
  * `get_state(key)` / higher-level queries.
* Accounts:

  * `get_balance(address)`
  * `get_nonce(address)`
* Staking:

  * `get_validators()`
  * `get_delegations(address)`
* Domains:

  * `list_domains()`
  * `get_domain(id)`
* Rollups:

  * `get_domain_head(domain_id)`
  * `get_rollup_batch(batch_id)`
* Governance:

  * `get_proposals()`
  * `get_votes(proposal_id)`

Transaction submission:

* `send_raw_tx(tx_bytes)`
* WebSocket:

  * `subscribe_new_blocks`
  * `subscribe_events(filter)`

---

# 11. SDKs & DevEx

## 11.1 Protocol SDK (Rust)

For building:

* System modules.
* Domain templates.
* Node customizations.

APIs:

* `StateAccess` trait (`get`, `set`, `iterate`).
* `Module` trait:

  * `init`, `handle_tx`, `handle_block_begin`, `handle_block_end`.
* Cryptographic primitives (hashes, signatures, ZK helpers).

## 11.2 Domain Template SDK

Simplify creation of new domains:

* Rust macros or config DSL:

  * `define_domain! { type: EVM, security_model: SHARED_SECURITY, ... }`
* Tools for:

  * generating domain genesis,
  * binding to sequencer,
  * registering in L1 domain registry.

## 11.3 dApp SDK (TypeScript)

For frontend/backend dApps:

* TypeScript client library (`@xchain/sdk`):

  * Key management (in browser).
  * Transaction building/signing.
  * Queries (RPC and indexer).
  * Helpers for:

    * bridging assets,
    * cross-domain messaging,
    * privacy actions (deposit/withdraw).

---

# 12. Indexer & Explorer

## 12.1 Indexer Service

Implement an **off-chain indexer** consuming node events:

* Ingest:

  * Blocks, txs, events, domain batches.
* Store into Postgres.
* Expose GraphQL/REST:

  * `blocks`, `transactions`, `accounts`, `domains`, `governance`, `rollup_batches`, `privacy_pool_stats`.

## 12.2 Explorer Backend

Build a small API on top of indexer to serve the Next.js frontend:

* Aggregated endpoints:

  * `/stats/chain`
  * `/stats/da`
  * `/stats/domains`
  * `/stats/sequencer`
  * `/stats/mixnet`

---

# 13. Next.js Frontend Spec

Implement a Next.js app in `frontend/apps/web` with:

## 13.1 Tech Stack

* Next.js App Router (TS).
* Tailwind + shadcn/ui for UI.
* React Query / TanStack Query for data fetching.
* `@xchain/sdk` for blockchain integration.
* SSR where useful (explorer), CSR for wallet interactions.

## 13.2 Core Routes

1. `/`

   * Overview dashboard:

     * L1 stats (TPS, block time, active validators).
     * DA stats (blob throughput).
     * Domains cards.
     * Testnet entry (connect wallet, get faucet X).
2. `/explorer`

   * Tabs: Blocks, Transactions, Accounts.
   * Block/tx detail pages.
3. `/domains`

   * List of domains with filters.
   * Domain detail:

     * type, security model, DA mode.
     * latest blocks / batches.
4. `/governance`

   * Proposals list.
   * Proposal detail:

     * description, on-chain code diff, voting chart.
   * Vote & delegate flows.
5. `/staking`

   * Validator list.
   * Delegate/undelegate flows.
6. `/wallet`

   * Balance, transactions, send/receive X.
   * Bridge UI:

     * deposit/withdraw between L1 and a domain.
   * Privacy tab:

     * shielded pool: deposit/withdraw.
7. `/dev`

   * Docs links.
   * API playground (call JSON-RPC).
   * Domain creation wizard (for advanced users).
8. `/sequencer`

   * Monitor sequencer health, batches, liveness.
9. `/testnet`

   * Faucet (CAPTCHA + rate limiting).
   * Quick start: run node, deploy contract, etc.

## 13.3 Frontend Integration Concerns

* Multi-endpoint support:

  * Node RPC,
  * Indexer API,
  * Sequencer API (for domain ops).
* Network switcher for:

  * Local devnet,
  * Public testnet,
  * Mainnet.
* Error handling:

  * fallback UIs for DA unavailability, domain paused, etc.

---

# 14. Environments & Pipelines

## 14.1 Environments

1. **Local Devnet**

   * `docker-compose`:

     * 4 validators + 1 DA light node + 1 sequencer + 1 indexer.
   * Pre-funded accounts.
2. **Internal Testnet**

   * Dev team nodes + CI testing.
   * Feature flags on.
3. **Public Testnet**

   * Faucet, explorers, docs.
   * Incentivized testing, “break it” campaigns.
4. **Mainnet Beta**

   * Limited gas limits, curated validators, tight governance.
   * Gradual parameter loosening.

## 14.2 CI/CD

* Every commit:

  * Unit tests (Rust + TS).
  * Property-based tests for state transitions.
  * Consensus simulation tests (multiple nodes).
* On tag:

  * Build Docker images for node, sequencer, indexer.
  * Generate changelog.
  * Publish SDKs to crates.io/npm.

---

# 15. Testing & Security

## 15.1 Testing Layers

* **Unit tests** for:

  * state transition functions,
  * consensus steps,
  * cryptographic primitives (with known vectors).
* **Property-based tests**:

  * random tx sequences; invariants (total supply, state consistency).
* **Consensus simulations**:

  * with network partitions, faulty leaders, crash & byzantine nodes.
* **DA sampling tests**:

  * simulate withholding attacks; ensure nodes reject chains without DA. ([Celestia][2])
* **Fuzzing**:

  * RPC inputs, tx decoders, VM opcodes.
* **Differential testing** for EVM domains:

  * compare against a reference client (e.g., go-ethereum) for same tx traces where applicable.

## 15.2 Audits & Bug Bounties

* Prioritize audits for:

  * consensus,
  * DA implementation,
  * bridges,
  * privacy circuits.
* Launch bug bounty in parallel with public testnet, with scoring rubric.

---

# 16. Implementation Order (for the “builder AI”)

Even though target is “full ecosystem”, build in this order, but **design with final architecture in mind**:

1. **Core libraries**

   * Cryptographic primitives.
   * Storage (state tree).
2. **Consensus skeleton**

   * Basic HotStuff engine.
   * In-memory state transition for `TRANSFER_X`.
3. **Node & RPC**

   * Single-node chain with basic RPC and `send_tx`.
4. **Data Availability**

   * DA blobs + commitments.
   * DA sampling for light nodes.
5. **Full PoS staking**

   * Validator set, staking txs, slashing rules.
6. **zkVM integration**

   * Prover/verifier for deterministic state transitions.
   * System contracts (staking, governance, domains).
7. **Domains v1**

   * EVM shared-security domain.
   * L1 <-> domain bridge.
8. **Shared sequencer v1**

   * Single sequencer.
   * Batching and posting to L1 DA.
9. **Cross-domain messaging v1**

   * Internal IBC-like messaging with light-client verification.
10. **Privacy pools v1**

    * Shielded pool for X.
11. **Indexer + Explorer**
12. **Public Testnet**
13. **Additional domains (WASM, privacy domain, payment domain)**
14. **Governance & Upgrades v2** (full on-chain)
15. **Mainnet Beta**

---

## 17. Repo-to-Spec Mapping (what lives where)

* `blockchain/protocol/consensus`: HotStuff engine (`ConsensusEngine` trait, QCs, view/locked logic). Add slashing hooks + telemetry.
* `blockchain/protocol/state`: Sparse Merkle state, accounts/staking/domain roots/governance/fee pools structs.
* `blockchain/protocol/da`: DA blob commitments, sampling logic, proofs, rewards split.
* `blockchain/protocol/vm`: zkVM/zkEVM syscall surface + precompile registry; proof verifier adapters.
* `blockchain/protocol/runtime`: State transition functions and system contracts dispatch (staking, governance, domains registry, bridges, privacy pools). Invariants live here.
* `blockchain/protocol/networking`: Gossip, RPC plumbing for consensus/DA/tx propagation.
* `blockchain/protocol/node`: Binary; wires consensus + runtime + networking + DA sampling; exposes JSON-RPC/gRPC/WS.
* `blockchain/domains/*`: Domain templates (EVM/WASM/privacy/payment), bridge bindings, domain genesis generators.
* `blockchain/sequencer/*`: Shared sequencer core + API + coordinator; batch builder posting DA blobs.
* `blockchain/contracts/*`: L1 contracts (staking, governance, domains_registry, rollup_bridge, privacy_pools, l1).
* `blockchain/indexer/*`: Rust ingest (`indexer-core`) + API (`api/`) with Postgres schema.
* `blockchain/sdk/*`: Rust SDK (protocol modules), TS SDK (`@xchain/sdk`), Python SDK minimal bindings.
* `blockchain/frontend/apps/web`: Next.js App Router routes aligned to §13.
* `blockchain/ops/docker`: `docker-compose.devnet.yml`, `genesis.json`, Dockerfiles.
* `docs/*`: Specs (this file, blueprint, extended spec).

## 18. Acceptance Criteria per Phase

* Core libs: hash functions, SM tree ops unit-tested; supply invariant holds for `TRANSFER_X`.
* Consensus skeleton: single-node & 4-node HotStuff happy-path finalizes; view-change on timeout; commit queue reflects 3-chain.
* Node & RPC: `send_tx` + `get_block/tx/state` roundtrip; deterministic state root for replayed blocks.
* DA sampling: Light node refuses chain when sampled chunks unavailable; DA proof verification path tested.
* Staking: Validator set updates applied at epoch boundaries; slashing triggers state change + evidence record.
* zkVM integration: `verify_zk_proof` syscall wired; mock verifier passes vectors; system contracts callable.
* Domains v1: EVM domain registered; rollup bridge deposit/withdraw paths update L1 + domain roots.
* Sequencer v1: `/v1/submit_tx`, `/v1/domain_head`, `/v1/batch_status` live; force-inclusion contract path callable.
* X-domain messaging v1: Packet format enforced; light-client verification on receipt; replay protected.
* Privacy pools v1: Deposit/withdraw circuits compiled; nullifier set updated; Merkle root matches commitments.
* Indexer + Explorer: Blocks/txs/domains/governance ingested; stats endpoints return non-empty data.
* Public testnet: Faucet dispenses X with rate limit; network switcher selects endpoints; observability dashboards live.

## 19. Config, Constants, and Ports

* `ops/docker/docker-compose.devnet.yml`: services = 4 validators, 1 DA light, sequencer, indexer+API+Postgres, frontend, mixnet stub.
* `ops/docker/genesis.json`: chain_id, initial validators, supply, params (block time, max gas, DA samples, slashing fractions).
* Fee splits (governance-controlled constants; default): `l1_gas { burn 30%, validators 70% }`, `da_fees { validators 70%, da_nodes 20%, treasury 10% }`, `l2_fees { sequencer 50%, da_costs 30%, l1_rent 20% }`.
* Default ports: node RPC 8545/26657-style, gRPC 9090, WS 8546, sequencer API 7545, indexer API 4000, frontend 3000.

## 20. Interfaces Snapshot (ground truth for stubs)

* Consensus (`protocol/consensus`):
  * `ConsensusEngine::{propose, vote, on_qc, on_timeout, validator_set, pop_commit, leader_for_view, current_view}`.
  * Add: `record_slash(evidence)`, `metrics()` hooks.
* Runtime/system contracts (`protocol/runtime`):
  * `Module::{init, handle_tx, handle_block_begin, handle_block_end}`.
  * Built-ins: staking, governance, domains_registry, rollup_bridge, privacy_pools.
* DA (`protocol/da`):
  * `SubmitBlob(domain_id, bytes) -> BlobId`, `GetBlob(id)`, `ProveBlobAvailability(id)`.
* VM (`protocol/vm`):
  * Syscalls: `read_state`, `write_state`, `emit_event`, `call_precompile(id, input)`, `verify_zk_proof(proof, vk)`.
  * Precompile registry: hashes, curves, sigs, ZK helpers, privacy primitives.
* Sequencer API (`sequencer/api`):
  * `POST /v1/submit_tx { domain_id, tx_bytes, fees, nonce }`
  * `GET /v1/domain_head?domain_id=`
  * `GET /v1/batch_status?domain_id=&batch_id=`
* Node RPC (JSON-RPC/gRPC/WS):
  * `get_block`, `get_tx`, `get_state`, `get_balance`, `get_nonce`, `get_validators`, `get_delegations`, `list_domains`, `get_domain`, `get_domain_head`, `get_rollup_batch`, `get_proposals`, `get_votes`, `send_raw_tx`, `subscribe_new_blocks`, `subscribe_events`.
* SDK TS (`sdk/sdk-ts`):
  * Wallet key mgmt, tx builder/sign, RPC + indexer clients, bridge helpers, x-domain messaging, privacy ops.
* Indexer API (`indexer/api`):
  * GraphQL/REST: `blocks`, `transactions`, `accounts`, `domains`, `governance`, `rollup_batches`, `privacy_pool_stats`; aggregated `/stats/{chain,da,domains,sequencer,mixnet}`.

## 21. Observability, Ops, and Safety Rails

* Metrics: consensus views/latency, block time, DA sampling success rate, sequencer batch latency, bridge queue depth, zk proof verify time, mixnet usage share.
* Logging: structured (json) with trace ids; redact private data; tag domain_id.
* Alerts: missed blocks/view-change spikes, DA sampling failures > threshold, sequencer lag, faucet depletion, indexer lag.
* Feature flags: enable/disable domains, zk verifier backends, mixnet routing.
* Escape hatches: force inclusion, domain pause, governance emergency veto; document operator runbooks.

### Reflection question

If you had to **pick one “anchor use case”** (e.g. cross-domain institutional payments, privacy-preserving B2B settlement, or generalized rollup platform), which would you choose as the narrative that this entire ecosystem is optimized for?

[1]: https://arxiv.org/abs/1803.05069?utm_source=chatgpt.com "HotStuff: BFT Consensus in the Lens of Blockchain"
[2]: https://celestia.org/glossary/data-availability-sampling/?utm_source=chatgpt.com "Data availability sampling - Celestia"
[3]: https://collective.flashbots.net/t/the-economics-of-shared-sequencing/2514?utm_source=chatgpt.com "The Economics of Shared Sequencing - Research"
[4]: https://nym.com/mixnet?utm_source=chatgpt.com "Noise Generating Mixnet"
