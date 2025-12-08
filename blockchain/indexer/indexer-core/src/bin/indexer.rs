use anyhow::Context;
use indexer_core::{BlockSink, PostgresSink};
use reqwest::StatusCode;
use runtime::Block;
use std::env;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let rpc_url = env::var("RPC_URL").unwrap_or_else(|_| "http://localhost:8545".to_string());
    let database_url =
        env::var("DATABASE_URL").context("DATABASE_URL env var is required for indexer")?;
    let start_height: u64 = env::var("START_HEIGHT")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap_or(0);
    let poll_ms: u64 = env::var("POLL_MS")
        .unwrap_or_else(|_| "2000".to_string())
        .parse()
        .unwrap_or(2000);
    let max_conn: u32 = env::var("DB_POOL_SIZE")
        .unwrap_or_else(|_| "5".to_string())
        .parse()
        .unwrap_or(5);

    info!(
        "starting indexer rpc_url={} start_height={} poll_ms={}",
        rpc_url, start_height, poll_ms
    );

    let client = reqwest::Client::new();
    let mut sink = PostgresSink::connect(&database_url, max_conn).await?;
    let mut height = start_height;

    loop {
        match fetch_block(&client, &rpc_url, height).await {
            Ok(Some(block)) => {
                info!("ingesting block height={}", height);
                if let Err(err) = sink.ingest_block(block).await {
                    error!("failed to ingest block {}: {err}", height);
                    sleep(Duration::from_millis(poll_ms)).await;
                    continue;
                }
                height += 1;
            }
            Ok(None) => {
                sleep(Duration::from_millis(poll_ms)).await;
            }
            Err(err) => {
                warn!("fetch error at height {}: {err}", height);
                sleep(Duration::from_millis(poll_ms)).await;
            }
        }
    }
}

async fn fetch_block(
    client: &reqwest::Client,
    rpc_url: &str,
    height: u64,
) -> anyhow::Result<Option<Block>> {
    let url = format!("{}/get_block/{}", rpc_url, height);
    let res = client.get(url).send().await?;
    if res.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let block_opt: Option<Block> = res.json().await?;
    Ok(block_opt)
}
