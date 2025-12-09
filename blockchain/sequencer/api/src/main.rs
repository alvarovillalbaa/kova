use axum::{extract::Query, routing::get, routing::post, Json, Router};
use runtime::Tx;
use serde::{Deserialize, Serialize};
use sequencer_core::{
    BatchStatus, RotationPolicy, Sequencer, SequencedBatch, SequencerInfo, SequencerSet,
};
use std::sync::Arc;
use std::env;
use tokio::sync::RwLock;
use tracing::info;
use zk_core::ZkBackend;
use zk_sp1::{Sp1Backend, Sp1Config, Sp1Program};
use zk_program_rollup;
use zk_program_privacy;
use std::fs;

#[derive(Clone)]
struct ApiState<S: Sequencer> {
    sequencer: Arc<RwLock<S>>,
    sequencer_set: Option<Arc<SequencerSet>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SubmitRequest {
    domain_id: String,
    tx: Tx,
}

async fn submit_tx<S: Sequencer>(state: Arc<ApiState<S>>, Json(req): Json<SubmitRequest>) -> Json<&'static str> {
    let mut seq = state.sequencer.write().await;
    seq.submit_tx(&req.domain_id, req.tx).await.unwrap();
    Json("ok")
}

#[derive(Debug, Deserialize)]
struct DomainQuery {
    domain_id: String,
}

async fn domain_head<S: Sequencer>(
    state: Arc<ApiState<S>>,
    Query(q): Query<DomainQuery>,
) -> Json<u64> {
    let seq = state.sequencer.read().await;
    let head = seq.domain_head(&q.domain_id).await.unwrap_or(0);
    Json(head)
}

async fn batch_status<S: Sequencer>(
    state: Arc<ApiState<S>>,
    Query(q): Query<BatchQuery>,
) -> Json<Option<BatchStatus>> {
    let seq = state.sequencer.read().await;
    let status = seq
        .batch_status(&q.domain_id, &q.batch_id)
        .await
        .unwrap_or(None);
    Json(status)
}

#[derive(Debug, Deserialize)]
struct BatchQuery {
    domain_id: String,
    batch_id: String,
}

#[derive(Debug, Serialize)]
struct ActiveSequencerResponse {
    active: Option<SequencerInfo>,
}

#[derive(Debug, Deserialize)]
struct ForceIncludeRequest {
    blob_id: String,
}

fn app<S: Sequencer + 'static>(state: ApiState<S>) -> Router {
    Router::new()
        .route("/v1/submit_tx", post(move |body| submit_tx(Arc::new(state.clone()), body)))
        .route("/v1/domain_head", get(move |q| domain_head(Arc::new(state.clone()), q)))
        .route("/v1/batch_status", get(move |q| batch_status(Arc::new(state.clone()), q)))
        .route(
            "/v1/active_sequencer",
            get({
                let state = state.clone();
                move || {
                    let state = state.clone();
                    async move {
                        let active = state
                            .sequencer_set
                            .as_ref()
                            .and_then(|s| s.active_leader(0));
                        Json(ActiveSequencerResponse { active })
                    }
                }
            }),
        )
        .route(
            "/v1/force_include",
            post({
                let state = state.clone();
                move |Json(body): Json<ForceIncludeRequest>| {
                    let state = state.clone();
                    async move {
                        if let Some(set) = state.sequencer_set.as_ref() {
                            set.enqueue_force_include(body.blob_id);
                            Json("queued")
                        } else {
                            Json("no sequencer set configured")
                        }
                    }
                }
            }),
        )
}

fn init_zk_backend() -> Option<Arc<dyn ZkBackend>> {
    let enabled = env::var("ENABLE_ZK").unwrap_or_else(|_| "0".into());
    if enabled != "1" && enabled.to_lowercase() != "true" {
        return None;
    }
    let rollup_elf = load_elf("ZK_SP1_ROLLUP_ELF", "zk/artifacts/rollup.elf");
    let privacy_elf = load_elf("ZK_SP1_PRIVACY_ELF", "zk/artifacts/privacy.elf");
    let programs = vec![
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
            tracing::warn!("unable to read {} ({}): {}", env_key, path, err);
            None
        }
    }
}

fn build_sequencer_set_from_env() -> Option<Arc<SequencerSet>> {
    let members = env::var("SEQUENCERS").ok()?;
    let roster: Vec<SequencerInfo> = members
        .split(',')
        .map(|id| SequencerInfo {
            id: id.trim().to_string(),
            stake: 1,
            endpoint: format!("http://{}", id.trim()),
        })
        .collect();
    Some(Arc::new(SequencerSet::new(roster, RotationPolicy::RoundRobin)))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let zk_backend = init_zk_backend();
    let sequencer_set = build_sequencer_set_from_env();
    let sequencer = sequencer_core::InMemorySequencer {
        pending: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        da: da::InMemoryDA::new(),
        batches: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        heads: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        zk: zk_backend,
    };
    let state = ApiState {
        sequencer: Arc::new(RwLock::new(sequencer)),
        sequencer_set,
    };
    let router = app(state);
    info!("sequencer api listening on 0.0.0.0:7545");
    axum::Server::bind(&"0.0.0.0:7545".parse().unwrap())
        .serve(router.into_make_service())
        .await
        .unwrap();
}

