use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use consensus::{ConsensusEngine, HotStuffEngine};
use da::InMemoryDA;
use networking::{ConsensusMessage, ConsensusNetwork, NoopConsensusNetwork};
use runtime::{
    apply_block, bootstrap_state, hash_block, load_genesis_from_file, Block, BlockHeader,
    ExecutionContext, Hash, Tx,
};
use serde::{Deserialize, Serialize};
use state::{ChainState, InMemoryStateStore, Validator};
use std::env;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};
use tracing::{info, warn};

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

    let chain_state = genesis_ctx.state.get_chain_state().await?;
    let validators: Vec<Validator> = chain_state.validators.values().cloned().collect();
    let local_validator = pick_local_validator(&chain_state);

    let consensus = HotStuffEngine::new(validators.clone());
    let node = Node {
        id: node_id,
        consensus: consensus.clone(),
        da: InMemoryDA::new(),
        state: genesis_ctx,
        blocks: Arc::new(Mutex::new(Vec::new())),
        mempool: Arc::new(Mutex::new(Vec::new())),
        local_validator,
        network: Arc::new(NoopConsensusNetwork::default()),
        tx_index: Arc::new(Mutex::new(HashMap::new())),
    };

    let proposer = spawn_block_production(node.clone());
    tokio::spawn(consensus.clone().run_timeouts());

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
    let server = axum::Server::bind(&addr).serve(app.into_make_service());

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
                match apply_block(&node.state, &block).await {
                    Ok(result) => {
                        let mut sealed = block.clone();
                        sealed.header.state_root = result.state_root;
                        sealed.header.gas_used = result.gas_used;
                        let mut blocks = node.blocks.lock().unwrap();
                        blocks.push(sealed.clone());
                        index_txs(&node, &sealed);
                        let block_id = hash_block(&sealed);
                        let _ = node.consensus.propose(sealed.clone()).await;
                        if let Some(validator) = node.local_validator.clone() {
                            let _ = node
                                .consensus
                                .vote(block_id, node.consensus.current_view(), &validator)
                                .await;
                        }
                        node.network.broadcast(ConsensusMessage::Propose(sealed.clone()));
                        while let Some(committed) = node.consensus.pop_commit() {
                            info!("commit block {:?}", committed);
                        }
                    }
                    Err(err) => warn!("failed to apply block: {err}"),
                }
            }
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

fn now_millis() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    now.as_millis() as u64
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

fn index_txs(node: &Node, block: &Block) {
    let mut index = node.tx_index.lock().unwrap();
    for tx in &block.transactions {
        index.insert(tx_hash(tx), (tx.clone(), block.header.height));
    }
}

