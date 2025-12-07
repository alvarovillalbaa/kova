use sequencer_core::{Sequencer, SequencedBatch};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    info!("sequencer coordinator scaffold starting");
    Ok(())
}

