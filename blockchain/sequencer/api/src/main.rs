use axum::{extract::Query, routing::get, routing::post, Json, Router};
use runtime::Tx;
use serde::{Deserialize, Serialize};
use sequencer_core::{BatchStatus, Sequencer, SequencedBatch};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone)]
struct ApiState<S: Sequencer> {
    sequencer: Arc<RwLock<S>>,
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

fn app<S: Sequencer + 'static>(state: ApiState<S>) -> Router {
    Router::new()
        .route("/v1/submit_tx", post(move |body| submit_tx(Arc::new(state.clone()), body)))
        .route("/v1/domain_head", get(move |q| domain_head(Arc::new(state.clone()), q)))
        .route("/v1/batch_status", get(move |q| batch_status(Arc::new(state.clone()), q)))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let sequencer = sequencer_core::InMemorySequencer {
        pending: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
        da: da::InMemoryDA::new(),
        batches: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        heads: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };
    let state = ApiState {
        sequencer: Arc::new(RwLock::new(sequencer)),
    };
    let router = app(state);
    info!("sequencer api listening on 0.0.0.0:7545");
    axum::Server::bind(&"0.0.0.0:7545".parse().unwrap())
        .serve(router.into_make_service())
        .await
        .unwrap();
}

