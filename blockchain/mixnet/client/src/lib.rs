use reqwest::Client;
use tracing::info;

pub async fn send_via_mixnet(endpoint: &str, payload: &[u8]) -> anyhow::Result<()> {
    info!("mixnet stub sending to {}", endpoint);
    let _ = Client::new().post(endpoint).body(payload.to_vec()).send().await?;
    Ok(())
}

