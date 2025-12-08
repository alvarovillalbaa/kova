use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use consensus::{sign_proposal, sign_vote, ConsensusEngine, HotStuffEngine, SignedProposal, SignedVote};
use da::{DAProvider, InMemoryDA};
use networking::{ConsensusMessage, ConsensusNetwork, NoopConsensusNetwork};
use runtime::{
    address_from_pubkey, apply_block, bootstrap_state, hash_block, load_genesis_from_file, verify_tx_signature,
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
use tokio::sync::broadcast;
use tracing::{info, warn};
use ed25519_dalek::SigningKey;
use blake3;
use uuid::Uuid;

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
    applied: Arc<Mutex<HashSet<Hash>>>,
    signing_key: Arc<SigningKey>,
    verifying_key: Vec<u8>,
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
}

#[derive(Serialize)]
struct Status {
    height: u64,
    mempool_len: usize,
    view: u64,
}

#[derive(Deserialize)]
struct TxRequest {
    tx: Tx,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let node_id = env::var("NODE_ID").unwrap_or_else(|_| "node-0".into());
    info!("kova node starting ({})", node_id);

    let genesis_ctx = if let Ok(path) = env::var("GENESIS_PATH") {
        info!("loading genesis from {}", path);
        load_genesis_from_file(path)?
    } else {
        bootstrap_state()
    };

    let node = create_node_with(
        &node_id,
        genesis_ctx,
        InMemoryDA::new(),
        Arc::new(NoopConsensusNetwork::default()),
    )
    .await?;

    let proposer = spawn_block_production(node.clone());
    tokio::spawn(node.consensus.clone().run_timeouts());

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
            "/send_raw_tx",
            post({
                let node = node.clone();
                move |Json(body): Json<TxRequest>| {
                    let node = node.clone();
                    async move {
                        if verify_tx_signature(&body.tx).is_err() {
                            return Json("invalid signature");
                        }
                        node.mempool.lock().unwrap().push(body.tx);
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

async fn process_commits(node: &Node) {
    while let Some(committed) = node.consensus.pop_commit() {
        info!("commit block {:?}", hex::encode(committed));
    }
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
    let da_root = blob
        .as_ref()
        .map(|b| blake3::hash(b.id.as_bytes()))
        .map(|h| *h.as_bytes())
        .unwrap_or([0u8; 32]);

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
        da_root,
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
        let expected = blake3::hash(blob_id.as_bytes());
        if sealed.header.da_root != *expected.as_bytes() {
            anyhow::bail!("da_root mismatch");
        }
    }

    let result = apply_block(&node.state, &sealed).await?;
    if sealed.header.state_root != [0u8; 32] && sealed.header.state_root != result.state_root {
        anyhow::bail!("state root mismatch for block");
    }
    sealed.header.state_root = result.state_root;
    sealed.header.gas_used = result.gas_used;

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
    index_txs(node, &sealed);
    Ok((sealed, block_id))
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
        applied: Arc::new(Mutex::new(HashSet::new())),
        signing_key,
        verifying_key,
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

        let node1 = create_node_with(node1_id, ctx1, da.clone(), network.clone()).await?;
        let node2 = create_node_with(node2_id, ctx2, da.clone(), network.clone()).await?;

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

