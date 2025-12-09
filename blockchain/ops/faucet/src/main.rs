use std::{env, net::SocketAddr, sync::Arc};

use axum::{extract::State, routing::{get, post}, Json, Router};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use sdk_rust::build_transfer_signed;
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    rpc: String,
    chain_id: String,
    default_amount: u128,
    signing_key: Arc<SigningKey>,
}

#[derive(Debug, Deserialize)]
struct FundRequest {
    address: String,
    #[serde(default)]
    amount: Option<u128>,
    #[serde(default)]
    nonce: Option<u64>,
}

#[derive(Debug, Serialize)]
struct FundResponse {
    status: u16,
    message: String,
}

fn parse_address(hex_addr: &str) -> anyhow::Result<[u8; 32]> {
    let mut out = [0u8; 32];
    let cleaned = hex_addr.trim_start_matches("0x");
    let bytes = hex::decode(cleaned)?;
    for (i, b) in bytes.iter().take(32).enumerate() {
        out[i] = *b;
    }
    Ok(out)
}

async fn fund(
    State(state): State<AppState>,
    Json(req): Json<FundRequest>,
) -> Result<Json<FundResponse>, (axum::http::StatusCode, String)> {
    let amount = req.amount.unwrap_or(state.default_amount);
    let nonce = req.nonce.unwrap_or(0);
    let addr = parse_address(&req.address)
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;

    let tx = build_transfer_signed(&state.chain_id, addr, amount, &state.signing_key, nonce)
        .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e.to_string()))?;

    let client = reqwest::Client::new();
    let url = format!("{}/send_raw_tx", state.rpc.trim_end_matches('/'));
    let res = client
        .post(&url)
        .json(&serde_json::json!({ "tx": tx }))
        .send()
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e.to_string()))?;

    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        warn!("faucet send_raw_tx failed: {} {}", status, body);
        return Err((axum::http::StatusCode::BAD_GATEWAY, body));
    }

    Ok(Json(FundResponse {
        status: status.as_u16(),
        message: if body.is_empty() { "sent".into() } else { body },
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let rpc = env::var("RPC_URL").unwrap_or_else(|_| "http://validator1:8545".into());
    let chain_id = env::var("CHAIN_ID").unwrap_or_else(|_| "kova-devnet".into());
    let default_amount = env::var("FAUCET_AMOUNT")
        .ok()
        .and_then(|v| v.parse::<u128>().ok())
        .unwrap_or(100_000);

    let sk_hex = env::var("FAUCET_SK")
        .map_err(|_| anyhow::anyhow!("FAUCET_SK env var (hex ed25519 key) required"))?;
    let sk_bytes = hex::decode(sk_hex.trim_start_matches("0x"))?;
    let signing_key = SigningKey::from_bytes(
        sk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("FAUCET_SK must be 32 bytes"))?,
    );

    let state = AppState {
        rpc,
        chain_id,
        default_amount,
        signing_key: Arc::new(signing_key),
    };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/fund", post(fund))
        .with_state(state);

    let addr: SocketAddr = env::var("FAUCET_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    info!("starting faucet on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
