use sequencer_core::{RotationPolicy, SequencerInfo, SequencerSet, Sequencer, SequencedBatch};
use tokio::time::{sleep, Duration};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let members = std::env::var("SEQUENCERS").unwrap_or_else(|_| "sequencer-0".into());
    let roster: Vec<SequencerInfo> = members
        .split(',')
        .enumerate()
        .map(|(idx, id)| SequencerInfo {
            id: id.trim().to_string(),
            stake: 1,
            endpoint: format!("http://{}", id.trim()),
        })
        .collect();
    let set = SequencerSet::new(roster, RotationPolicy::RoundRobin);
    let member_count = set.member_count();
    info!(
        "sequencer coordinator starting with {} configured members",
        member_count
    );

    let mut round: u64 = 0;
    loop {
        if let Some(active) = set.active_leader(round) {
            info!("round {} active sequencer: {}", round, active.id);
        } else {
            info!("round {} no active sequencer configured", round);
        }
        if let Some(force_blob) = set.pop_force_include() {
            info!("force-include requested for blob {}", force_blob);
        }
        round = round.saturating_add(1);
        sleep(Duration::from_secs(5)).await;
    }
    Ok(())
}

