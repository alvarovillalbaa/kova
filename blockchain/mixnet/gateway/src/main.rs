use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
struct RelayRequest {
    endpoint: String,
    payload: String,
}

async fn relay(Json(req): Json<RelayRequest>) -> Json<&'static str> {
    info!("relaying to {} via mixnet stub", req.endpoint);
    Json("relayed")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let app = Router::new().route("/relay", post(relay));
    info!("mixnet gateway listening on 0.0.0.0:8050");
    axum::Server::bind(&"0.0.0.0:8050".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

