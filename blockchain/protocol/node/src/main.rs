use axum::{
    extract::{Path, Query},
    routing::{get, post},
    Json, Router,
};
use consensus::{sign_proposal, sign_vote, ConsensusEngine, HotStuffEngine, SignedProposal, SignedVote};
use da::{DAProvider, InMemoryDA, verify_da_proof};
use networking::{parse_multiaddr_list, start_libp2p_consensus, ConsensusMessage, ConsensusNetwork, NoopConsensusNetwork};
use runtime::{
    address_from_pubkey, apply_block, bootstrap_state, hash_block, load_genesis_from_file, verify_signature_bytes,
    verify_tx_signature,
    Block, BlockHeader, ExecutionContext, Hash, Tx,
};
use serde::{Deserialize, Serialize};
use state::{ChainState, InMemoryStateStore, StateStore, Validator, ValidatorStatus};
use std::env;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, warn};
use ed25519_dalek::SigningKey;
use libp2p::{identity, Multiaddr};
use blake3;
use uuid::Uuid;
use zk_core::{BlockProof, ProgramId, ProofRequest, ZkBackend};
use zk_program_block;
use zk_program_privacy;
use zk_program_rollup;
use zk_sp1::{Sp1Backend, Sp1Config, Sp1Program};
use std::fs;

const MEMPOOL_LIMIT: usize = 10_000;

#[derive(Clone)]
struct Node {
    id: String,
    consensus: HotStuffEngine,
    da: InMemoryDA,
    state: ExecutionContext<InMemoryStateStore>,
    blocks: Arc<Mutex<Vec<Block>>>,
    mempool: Arc<Mutex<Vec<Tx>>>,
    local_validator: Option<Validator>,
    network: Arc<dyn ConsensusNetwork + Send + Sync>,
    tx_index: Arc<Mutex<HashMap<Hash, (Tx, u64)>>>,
    block_store: Arc<Mutex<HashMap<Hash, Block>>>,
    block_proofs: Arc<Mutex<HashMap<Hash, BlockProof>>>,
    applied: Arc<Mutex<HashSet<Hash>>>,
    signing_key: Arc<SigningKey>,
    verifying_key: Vec<u8>,
    zk: Option<Arc<dyn ZkBackend>>,
}

#[derive(Clone)]
struct LocalBus {
    tx: broadcast::Sender<ConsensusMessage>,
}

impl LocalBus {
    fn new(capacity: usize) -> (Self, broadcast::Receiver<ConsensusMessage>) {
        let (tx, rx) = broadcast::channel(capacity);
        (Self { tx }, rx)
    }

    fn subscribe(&self) -> broadcast::Receiver<ConsensusMessage> {
        self.tx.subscribe()
    }
}

impl ConsensusNetwork for LocalBus {
    fn broadcast(&self, msg: ConsensusMessage) {
        let _ = self.tx.send(msg);
    }

    fn broadcast_tx(&self, _tx: &Tx) {
        // in-process only; tx gossip handled via libp2p
    }
}

fn default_listen_addr() -> Multiaddr {
    "/ip4/0.0.0.0/udp/9000/quic-v1"
        .parse()
        .unwrap_or_else(|_| "/ip4/0.0.0.0/udp/9000/quic-v1".parse().unwrap())
}

async fn init_consensus_network(
    node_id: &str,
) -> (
    Arc<dyn ConsensusNetwork + Send + Sync>,
    Option<mpsc::Receiver<ConsensusMessage>>,
    Option<mpsc::Receiver<Tx>>,
) {
    let listen = env::var("P2P_LISTEN").unwrap_or_else(|_| "/ip4/0.0.0.0/udp/9000/quic-v1".into());
    let listen_addr: Multiaddr = listen.parse().unwrap_or_else(|_| default_listen_addr());
    let bootstrap = env::var("P2P_BOOTSTRAP").unwrap_or_default();
    let seed = derive_signing_key(node_id).to_bytes();
    let keypair = identity::Keypair::ed25519_from_bytes(seed.to_vec())
        .unwrap_or_else(|_| identity::Keypair::generate_ed25519());
    match start_libp2p_consensus(keypair, listen_addr, parse_multiaddr_list(&bootstrap)).await {
        Ok((net, consensus_rx, tx_rx)) => (
            net as Arc<dyn ConsensusNetwork + Send + Sync>,
            Some(consensus_rx),
            Some(tx_rx),
        ),
        Err(err) => {
            warn!("libp2p consensus fallback to noop: {err}");
            (Arc::new(NoopConsensusNetwork::default()), None, None)
        }
    }
}

#[derive(Serialize)]
struct Status {
    height: u64,
    mempool_len: usize,
    view: u64,
}

fn init_zk_backend() -> Option<Arc<dyn ZkBackend>> {
    let enabled = env::var("ENABLE_ZK").unwrap_or_else(|_| "0".into());
    if enabled != "1" && enabled.to_lowercase() != "true" {
        return None;
    }
    let block_elf = load_elf("ZK_SP1_BLOCK_ELF", "zk/artifacts/block.elf");
    let rollup_elf = load_elf("ZK_SP1_ROLLUP_ELF", "zk/artifacts/rollup.elf");
    let privacy_elf = load_elf("ZK_SP1_PRIVACY_ELF", "zk/artifacts/privacy.elf");

    let programs = vec![
        Sp1Program {
            id: zk_program_block::program_id(),
            elf: block_elf.unwrap_or_default(),
            name: "block_transition".into(),
            version: "0.1.0",
        },
        Sp1Program {
            id: zk_program_rollup::program_id(),
            elf: rollup_elf.unwrap_or_default(),
            name: "rollup_batch".into(),
            version: "0.1.0",
        },
        Sp1Program {
            id: zk_program_privacy::program_id(),
            elf: privacy_elf.unwrap_or_default(),
            name: "privacy_withdraw".into(),
            version: "0.1.0",
        },
    ];
    let backend = Sp1Backend::new(Sp1Config {
        programs,
        verify_only: false,
    });
    Some(Arc::new(backend))
}

fn load_elf(env_key: &str, default_path: &str) -> Option<Vec<u8>> {
    let path = env::var(env_key).unwrap_or_else(|_| default_path.into());
    match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(err) => {
            warn!("unable to read {} ({}): {}", env_key, path, err);
            None
        }
    }
}

#[derive(Deserialize)]
struct TxRequest {
    tx: Tx,
}

#[derive(Deserialize)]
struct SampleQuery {
    blob_id: String,
    samples: Option<usize>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let node_id = env::var("NODE_ID").unwrap_or_else(|_| "node-0".into());
    info!("kova node starting ({})", node_id);

    let zk_backend = init_zk_backend();

    let genesis_ctx = if let Ok(path) = env::var("GENESIS_PATH") {
        info!("loading genesis from {}", path);
        load_genesis_from_file(path)?
    } else {
        bootstrap_state()
    }
    .with_zk(zk_backend.clone());

    let (network, consensus_rx, tx_rx) = init_consensus_network(&node_id).await;

    let node = create_node_with(
        &node_id,
        genesis_ctx,
        InMemoryDA::new(),
        network.clone(),
        zk_backend.clone(),
    )
    .await?;

    let proposer = spawn_block_production(node.clone());
    tokio::spawn(node.consensus.clone().run_timeouts());
    if let Some(rx) = consensus_rx {
        spawn_p2p_consensus_listener(node.clone(), rx);
    }
    if let Some(rx) = tx_rx {
        spawn_tx_gossip_listener(node.clone(), rx);
    }

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/status",
            get({
                let node = node.clone();
                move || {
                    let node = node.clone();
                    async move {
                        let height = node.blocks.lock().unwrap().len() as u64;
                        let mempool_len = node.mempool.lock().unwrap().len();
                        let view = node.consensus.current_view();
                        Json(Status {
                            height,
                            mempool_len,
                            view,
                        })
                    }
                }
            }),
        )
        .route(
            "/governance/proposals",
            get({
                let node = node.clone();
                move || {
                    let node = node.clone();
                    async move {
                        let chain = node.state.state.get_chain_state().await.ok();
                        let proposals = chain.map(|c| c.proposals.values().cloned().collect::<Vec<_>>());
                        Json(proposals)
                    }
                }
            }),
        )
        .route(
            "/governance/proposal/:id",
            get({
                let node = node.clone();
                move |Path(id): Path<String>| {
                    let node = node.clone();
                    async move {
                        let Ok(uuid) = Uuid::parse_str(&id) else {
                            return Json(None::<state::Proposal>);
                        };
                        let proposal = node
                            .state
                            .state
                            .get_chain_state()
                            .await
                            .ok()
                            .and_then(|c| c.proposals.get(&uuid).cloned());
                        Json(proposal)
                    }
                }
            }),
        )
        .route(
            "/privacy/pool",
            get({
                let node = node.clone();
                move || {
                    let node = node.clone();
                    async move {
                        let pool = node
                            .state
                            .state
                            .get_chain_state()
                            .await
                            .ok()
                            .and_then(|c| c.privacy_pools.get("shielded").cloned());
                        Json(pool)
                    }
                }
            }),
        )
        .route(
            "/send_raw_tx",
            post({
                let node = node.clone();
                move |Json(body): Json<TxRequest>| {
                    let node = node.clone();
                    async move {
                        if verify_tx_signature(&body.tx).is_err() {
                            return Json("invalid signature");
                        }
                        enqueue_tx(&node, body.tx.clone());
                        node.network.broadcast_tx(&body.tx);
                        Json("ok")
                    }
                }
            }),
        )
        .route(
            "/get_block/:height",
            get({
                let node = node.clone();
                move |Path(height): Path<usize>| {
                    let node = node.clone();
                    async move {
                        let blocks = node.blocks.lock().unwrap();
                        let block = blocks.get(height).cloned();
                        Json(block)
                    }
                }
            }),
        )
        .route(
            "/da/commitment/:id",
            get({
                let node = node.clone();
                move |Path(id): Path<String>| {
                    let node = node.clone();
                    async move {
                        let commitment = node.da.get_commitment(&id).await.ok();
                        Json(commitment)
                    }
                }
            }),
        )
        .route(
            "/da/sample",
            get({
                let node = node.clone();
                move |Query(q): Query<SampleQuery>| {
                    let node = node.clone();
                    async move {
                        let ok = node
                            .da
                            .sample(&q.blob_id, q.samples.unwrap_or(2))
                            .await
                            .is_ok();
                        Json(ok)
                    }
                }
            }),
        )
        .route(
            "/block_proof/:height",
            get({
                let node = node.clone();
                move |Path(height): Path<usize>| {
                    let node = node.clone();
                    async move {
                        let blocks = node.blocks.lock().unwrap();
                        let block = blocks.get(height).cloned();
                        let proof = block.and_then(|b| {
                            let h = hash_block(&b);
                            node.block_proofs.lock().unwrap().get(&h).cloned()
                        });
                        Json(proof)
                    }
                }
            }),
        )
        .route(
            "/get_tx/:hash",
            get({
                let node = node.clone();
                move |Path(hash_hex): Path<String>| {
                    let node = node.clone();
                    async move {
                        let Ok(bytes) = hex::decode(hash_hex.strip_prefix("0x").unwrap_or(&hash_hex)) else {
                            return Json(None::<Tx>);
                        };
                        if bytes.len() != 32 {
                            return Json(None::<Tx>);
                        }
                        let mut h = [0u8; 32];
                        h.copy_from_slice(&bytes);
                        let tx_opt = node.tx_index.lock().unwrap().get(&h).cloned();
                        Json(tx_opt.map(|(tx, _)| tx))
                    }
                }
            }),
        )
        .route(
            "/get_balance/:address",
            get({
                let node = node.clone();
                move |Path(addr_hex): Path<String>| {
                    let node = node.clone();
                    async move {
                        let Some(address) = parse_address(&addr_hex) else {
                            return Json(None::<u128>);
                        };
                        let account = node
                            .state
                            .state
                            .get_account(&address)
                            .await
                            .ok()
                            .flatten();
                        Json(account.map(|a| a.balance_x))
                    }
                }
            }),
        )
        .route(
            "/get_nonce/:address",
            get({
                let node = node.clone();
                move |Path(addr_hex): Path<String>| {
                    let node = node.clone();
                    async move {
                        let Some(address) = parse_address(&addr_hex) else {
                            return Json(None::<u64>);
                        };
                        let account = node
                            .state
                            .state
                            .get_account(&address)
                            .await
                            .ok()
                            .flatten();
                        Json(account.map(|a| a.nonce))
                    }
                }
            }),
        )
        .route(
            "/get_validators",
            get({
                let node = node.clone();
                move || {
                    let node = node.clone();
                    async move {
                        let validators = node.consensus.validator_set().await.unwrap_or_default();
                        Json(validators)
                    }
                }
            }),
        );

    let addr: SocketAddr = "0.0.0.0:8545".parse()?;
    info!("RPC listening on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(listener, app.into_make_service());

    tokio::select! {
        _ = proposer => {}
        res = server => {
            if let Err(err) = res {
                warn!("server error: {err}");
            }
        }
    }
    Ok(())
}

fn spawn_block_production(node: Node) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(node.state.block_time_ms));
        loop {
            interval.tick().await;
            let is_leader = node
                .consensus
                .leader_for_view(node.consensus.current_view())
                .map(|v| {
                    node.local_validator
                        .as_ref()
                        .map(|me| me.id == v.id)
                        .unwrap_or(true)
                })
                .unwrap_or(true);

            if !is_leader {
                continue;
            }

            let maybe_block = build_block(&node).await;
            if let Some(block) = maybe_block {
                let view = node.consensus.current_view();
                match execute_and_record(&node, &block).await {
                    Ok((sealed, block_id)) => {
                        let proposal = SignedProposal {
                            block: sealed.clone(),
                            public_key: node.verifying_key.clone(),
                            signature: sign_proposal(&sealed, &node.signing_key),
                        };
                        if let Err(err) = node.consensus.propose(proposal.clone()).await {
                            warn!("proposal rejected: {err}");
                            continue;
                        }
                        node.network
                            .broadcast(ConsensusMessage::Propose(proposal.clone()));

                        if let Some(validator) = node.local_validator.clone() {
                            let vote = SignedVote {
                                block_id,
                                view,
                                voter: validator,
                                signature: sign_vote(&block_id, view, &node.signing_key),
                            };
                            let _ = node.consensus.vote(vote.clone()).await;
                            node.network.broadcast(ConsensusMessage::Vote(vote));
                        }
                        while let Some(committed) = node.consensus.pop_commit() {
                            info!("commit block {:?}", committed);
                        }
                    }
                    Err(err) => warn!("failed to build block: {err}"),
                }
            }
        }
    })
}

async fn handle_message(node: &Node, msg: ConsensusMessage) {
    if !verify_consensus_message(node, &msg).await {
        warn!("discarded invalid consensus message");
        return;
    }
    match msg {
        ConsensusMessage::Propose(proposal) => {
            if let Err(err) = node.consensus.propose(proposal.clone()).await {
                warn!("consensus rejected proposal: {err}");
                return;
            }
            if let Err(err) = execute_and_record(node, &proposal.block).await {
                warn!("failed to execute proposal: {err}");
                return;
            }
            if let Some(validator) = node.local_validator.clone() {
                let view = proposal
                    .block
                    .header
                    .consensus_metadata
                    .get("view")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(node.consensus.current_view());
                let block_id = hash_block(&proposal.block);
                let vote = SignedVote {
                    block_id,
                    view,
                    voter: validator,
                    signature: sign_vote(&block_id, view, &node.signing_key),
                };
                let _ = node.consensus.vote(vote.clone()).await;
                node.network.broadcast(ConsensusMessage::Vote(vote));
            }
        }
        ConsensusMessage::Vote(vote) => {
            if let Err(err) = node.consensus.vote(vote).await {
                warn!("vote rejected: {err}");
            }
        }
        ConsensusMessage::Timeout { view, .. } => {
            let _ = node.consensus.on_timeout(view).await;
        }
    }
    process_commits(node).await;
}

async fn verify_consensus_message(node: &Node, msg: &ConsensusMessage) -> bool {
    match msg {
        ConsensusMessage::Propose(p) => {
            if address_from_pubkey(&p.public_key) != p.block.header.proposer_id {
                return false;
            }
            let block_id = hash_block(&p.block);
            verify_signature_bytes(&p.public_key, &p.signature, block_id.as_slice()).is_ok()
        }
        ConsensusMessage::Vote(v) => {
            let validators = node.consensus.validator_set().await.unwrap_or_default();
            if let Some(expected) = validators.iter().find(|val| val.id == v.voter.id) {
                if expected.pubkey != v.voter.pubkey {
                    return false;
                }
            }
            let msg_bytes = bincode::serialize(&(v.block_id, v.view)).unwrap_or_default();
            verify_signature_bytes(&v.voter.pubkey, &v.signature, &msg_bytes).is_ok()
        }
        ConsensusMessage::Timeout { from, .. } => {
            let validators = node.consensus.validator_set().await.unwrap_or_default();
            validators.iter().any(|val| val.id == from.id)
        }
    }
}

async fn process_commits(node: &Node) {
    while let Some(committed) = node.consensus.pop_commit() {
        info!("commit block {:?}", hex::encode(committed));
    }
}

fn spawn_p2p_consensus_listener(
    node: Node,
    mut rx: mpsc::Receiver<ConsensusMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            handle_message(&node, msg).await;
        }
    })
}

fn spawn_tx_gossip_listener(node: Node, mut rx: mpsc::Receiver<Tx>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(tx) = rx.recv().await {
            enqueue_tx(&node, tx);
        }
    })
}

fn spawn_network_listener(node: Node, mut rx: broadcast::Receiver<ConsensusMessage>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            handle_message(&node, msg).await;
        }
    })
}

async fn build_block(node: &Node) -> Option<Block> {
    let txs = {
        let mut mempool = node.mempool.lock().unwrap();
        if mempool.is_empty() {
            return None;
        }
        mempool.sort_by(|a, b| tx_priority(b, node.state.base_fee).cmp(&tx_priority(a, node.state.base_fee)));
        mempool.drain(..).collect::<Vec<_>>()
    };

    let parent_hash = node
        .blocks
        .lock()
        .unwrap()
        .last()
        .map(hash_block)
        .unwrap_or([0u8; 32]);

    let blob = match serde_json::to_vec(&txs) {
        Ok(bytes) => node.da.submit_blob("l1", &bytes).await.ok(),
        Err(_) => None,
    };

    let proposer_id = node
        .local_validator
        .as_ref()
        .map(|v| v.owner)
        .unwrap_or([0u8; 32]);

    let l1_tx_root = tx_root(&txs);
    let height = node.blocks.lock().unwrap().len() as u64;
    let header = BlockHeader {
        parent_hash,
        height,
        timestamp: now_millis(),
        proposer_id,
        state_root: [0u8; 32],
        l1_tx_root,
        da_commitment: blob.as_ref().map(|b| runtime::BlockDACommitment {
            root: b.commitment.root,
            total_shards: b.commitment.total_shards as u32,
            data_shards: b.commitment.data_shards as u32,
            parity_shards: b.commitment.parity_shards as u32,
            shard_size: b.commitment.shard_size as u32,
        }),
        domain_roots: vec![],
        gas_used: 0,
        gas_limit: node.state.max_gas_per_block,
        base_fee: node.state.base_fee,
        consensus_metadata: serde_json::json!({
            "view": node.consensus.current_view()
        }),
    };

    Some(Block {
        header,
        transactions: txs,
        da_blobs: blob.map(|b| vec![b.id]).unwrap_or_default(),
    })
}

fn tx_root(txs: &[Tx]) -> [u8; 32] {
    let bytes = bincode::serialize(txs).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

fn tx_hash(tx: &Tx) -> [u8; 32] {
    let bytes = bincode::serialize(tx).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

fn tx_priority(tx: &Tx, base_fee: u128) -> u128 {
    if let Some(max_fee) = tx.max_fee {
        let priority = tx.max_priority_fee.unwrap_or(0);
        return max_fee.saturating_add(priority);
    }
    tx.gas_price.unwrap_or(base_fee)
}

fn enqueue_tx(node: &Node, tx: Tx) {
    if verify_tx_signature(&tx).is_err() {
        warn!("dropped tx with invalid signature");
        return;
    }
    let h = tx_hash(&tx);
    if node.tx_index.lock().unwrap().contains_key(&h) {
        return;
    }
    let mut mempool = node.mempool.lock().unwrap();
    if mempool.len() >= MEMPOOL_LIMIT {
        warn!("mempool full, dropping tx");
        return;
    }
    if mempool.iter().any(|existing| tx_hash(existing) == h) {
        return;
    }
    mempool.push(tx);
}

fn drop_included_txs(node: &Node, txs: &[Tx]) {
    let drop_hashes: HashSet<_> = txs.iter().map(tx_hash).collect();
    let mut mempool = node.mempool.lock().unwrap();
    mempool.retain(|t| !drop_hashes.contains(&tx_hash(t)));
}


async fn execute_and_record(node: &Node, block: &Block) -> anyhow::Result<(Block, Hash)> {
    let mut sealed = block.clone();
    let block_id = hash_block(&sealed);
    {
        let applied = node.applied.lock().unwrap();
        if applied.contains(&block_id) {
            return Ok((sealed, block_id));
        }
    }

    for blob_id in &sealed.da_blobs {
        let proof = node.da.prove_blob_availability(blob_id).await?;
        if proof.samples.is_empty() {
            anyhow::bail!("empty DA proof");
        }
        let Some(commitment) = sealed.header.da_commitment.as_ref() else {
            anyhow::bail!("missing da commitment in header");
        };
        if proof.commitment.root != commitment.root {
            anyhow::bail!("da commitment root mismatch");
        }
        if !verify_da_proof(&proof) {
            anyhow::bail!("invalid DA sampling proof");
        }
    }

    let result = apply_block(&node.state, &sealed).await?;
    if sealed.header.state_root != [0u8; 32] && sealed.header.state_root != result.state_root {
        anyhow::bail!("state root mismatch for block");
    }
    sealed.header.state_root = result.state_root;
    sealed.header.gas_used = result.gas_used;

    if let Some(zk) = node.zk.clone() {
        if let Err(err) = prove_block(node, zk, &sealed, &result, block_id).await {
            warn!("zk proof generation failed: {err}");
        }
    }

    {
        let mut store = node.block_store.lock().unwrap();
        store.insert(block_id, sealed.clone());
    }
    {
        let mut applied = node.applied.lock().unwrap();
        applied.insert(block_id);
    }
    {
        let mut chain = node.blocks.lock().unwrap();
        chain.push(sealed.clone());
    }
    drop_included_txs(node, &sealed.transactions);
    index_txs(node, &sealed);
    Ok((sealed, block_id))
}

async fn prove_block(
    node: &Node,
    zk: Arc<dyn ZkBackend>,
    block: &Block,
    result: &runtime::BlockApplyResult,
    block_id: Hash,
) -> anyhow::Result<()> {
    let events_root = zk_program_block::hash_events(&result.events);
    let witness =
        zk_program_block::encode_witness(block, result.state_root, &result.events, result.gas_used)?;
    let da_root = block
        .header
        .da_commitment
        .as_ref()
        .map(|c| c.root)
        .unwrap_or([0u8; 32]);
    let commitments = zk_program_block::commitments(result.state_root, events_root, da_root);
    let artifact = zk
        .prove(ProofRequest {
            program_id: ProgramId::Block,
            witness,
            commitments: Some(commitments),
        })
        .await
        .map_err(|e| anyhow::anyhow!("prove error: {e}"))?;
    zk.verify(&artifact)
        .await
        .map_err(|e| anyhow::anyhow!("verify error: {e}"))?;

    let record = BlockProof {
        block_hash: block_id,
        state_root: result.state_root,
        proof: artifact,
    };
    node.block_proofs.lock().unwrap().insert(block_id, record);
    Ok(())
}

fn now_millis() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    now.as_millis() as u64
}

fn derive_signing_key(node_id: &str) -> SigningKey {
    let digest = blake3::hash(node_id.as_bytes());
    SigningKey::from_bytes(digest.as_bytes())
}

async fn ensure_local_validator(
    ctx: &ExecutionContext<InMemoryStateStore>,
    verifying_key: &[u8],
) -> anyhow::Result<Validator> {
    let mut chain = ctx.state.get_chain_state().await?;
    let owner = address_from_pubkey(verifying_key);
    if let Some(v) = chain.validators.values().find(|v| v.owner == owner).cloned() {
        return Ok(v);
    }
    let id = Uuid::new_v5(&Uuid::NAMESPACE_OID, verifying_key);
    let validator = Validator {
        owner,
        id,
        pubkey: verifying_key.to_vec(),
        stake: 1_000,
        status: ValidatorStatus::Active,
        commission_rate: 0,
    };
    chain.validators.insert(id, validator.clone());
    ctx.state.put_chain_state(chain).await?;
    Ok(validator)
}

fn pick_local_validator(chain_state: &ChainState) -> Option<Validator> {
    if let Ok(hex_owner) = env::var("VALIDATOR_OWNER") {
        if let Ok(bytes) = hex::decode(hex_owner) {
            if bytes.len() == 32 {
                let mut addr = [0u8; 32];
                addr.copy_from_slice(&bytes);
                return chain_state
                    .validators
                    .values()
                    .find(|v| v.owner == addr)
                    .cloned();
            }
        }
    }
    chain_state.validators.values().next().cloned()
}

fn parse_address(hex_str: &str) -> Option<[u8; 32]> {
    let clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(clean).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut addr = [0u8; 32];
    addr.copy_from_slice(&bytes);
    Some(addr)
}

async fn create_node_with(
    node_id: &str,
    ctx: ExecutionContext<InMemoryStateStore>,
    da: InMemoryDA,
    network: Arc<dyn ConsensusNetwork + Send + Sync>,
    zk: Option<Arc<dyn ZkBackend>>,
) -> anyhow::Result<Node> {
    let signing_key = Arc::new(derive_signing_key(node_id));
    let verifying_key = signing_key.verifying_key().to_bytes().to_vec();
    let local_validator = ensure_local_validator(&ctx, &verifying_key).await?;
    let chain_state = ctx.state.get_chain_state().await?;
    let mut validators: Vec<Validator> = chain_state.validators.values().cloned().collect();
    validators.sort_by_key(|v| v.owner);
    let consensus = HotStuffEngine::new(validators.clone());
    Ok(Node {
        id: node_id.to_string(),
        consensus,
        da,
        state: ctx,
        blocks: Arc::new(Mutex::new(Vec::new())),
        mempool: Arc::new(Mutex::new(Vec::new())),
        local_validator: Some(local_validator),
        network,
        tx_index: Arc::new(Mutex::new(HashMap::new())),
        block_store: Arc::new(Mutex::new(HashMap::new())),
        block_proofs: Arc::new(Mutex::new(HashMap::new())),
        applied: Arc::new(Mutex::new(HashSet::new())),
        signing_key,
        verifying_key,
        zk,
    })
}

fn index_txs(node: &Node, block: &Block) {
    let mut index = node.tx_index.lock().unwrap();
    for tx in &block.transactions {
        index.insert(tx_hash(tx), (tx.clone(), block.header.height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{
        sign_bytes, tx_signing_bytes, FeeSplit, GenesisConfig, GenesisValidator, TxPayload,
    };
    use tokio::time::Duration;

    fn recipient_balance(node: &Node, addr: &runtime::Address) -> u128 {
        futures::executor::block_on(node.state.state.get_account(addr))
            .ok()
            .flatten()
            .map(|a| a.balance_x)
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn consensus_da_state_end_to_end() -> anyhow::Result<()> {
        let node1_id = "node-1";
        let node2_id = "node-2";
        let node1_sk = derive_signing_key(node1_id);
        let node2_sk = derive_signing_key(node2_id);
        let validators = vec![
            GenesisValidator {
                pubkey: node1_sk.verifying_key().to_bytes().to_vec(),
                stake: 1_000,
                commission_rate: 0,
            },
            GenesisValidator {
                pubkey: node2_sk.verifying_key().to_bytes().to_vec(),
                stake: 1_000,
                commission_rate: 0,
            },
        ];

        let user_sk = SigningKey::from_bytes(&[9u8; 32]);
        let user_pk = user_sk.verifying_key().to_bytes().to_vec();
        let user_addr = address_from_pubkey(&user_pk);
        let recipient = [5u8; 32];

        let genesis = GenesisConfig {
            chain_id: "kova-devnet".into(),
            initial_validators: validators,
            initial_accounts: vec![(user_addr, 1_000_000)],
            block_time_ms: 200,
            max_gas_per_block: 1_000_000,
            base_fee: 1,
            da_sample_count: 4,
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
        };

        let ctx1 = runtime::from_genesis(genesis.clone()).await?;
        let ctx2 = runtime::from_genesis(genesis).await?;

        let (bus, rx1) = LocalBus::new(1024);
        let rx2 = bus.subscribe();
        let network = Arc::new(bus.clone());
        let da = InMemoryDA::new();

        let node1 = create_node_with(node1_id, ctx1, da.clone(), network.clone(), None).await?;
        let node2 = create_node_with(node2_id, ctx2, da.clone(), network.clone(), None).await?;

        let listener1 = spawn_network_listener(node1.clone(), rx1);
        let listener2 = spawn_network_listener(node2.clone(), rx2);
        let producer1 = spawn_block_production(node1.clone());
        let producer2 = spawn_block_production(node2.clone());

        let mut tx = runtime::Tx {
            chain_id: "kova-devnet".into(),
            nonce: 0,
            gas_limit: 50_000,
            max_fee: Some(1),
            max_priority_fee: Some(0),
            gas_price: None,
            payload: TxPayload::Transfer { to: recipient, amount: 10 },
            public_key: user_pk.clone(),
            signature: vec![],
        };
        let msg = tx_signing_bytes(&tx)?;
        tx.signature = sign_bytes(&user_sk, &msg);
        node1.mempool.lock().unwrap().push(tx);

        tokio::time::sleep(Duration::from_millis(1_800)).await;

        let bal1 = recipient_balance(&node1, &recipient);
        let bal2 = recipient_balance(&node2, &recipient);

        producer1.abort();
        producer2.abort();
        listener1.abort();
        listener2.abort();

        assert!(node1.blocks.lock().unwrap().len() >= 1);
        assert!(node2.blocks.lock().unwrap().len() >= 1);
        assert_eq!(bal1, bal2);
        assert!(bal1 >= 10);
        Ok(())
    }
}

