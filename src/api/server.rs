use crate::api::handlers::{arrays, energy, health, inverters, tou, trueup};
use axum::{
    Router,
    routing::{get, post},
};
use sqlx::SqlitePool;
use std::collections::HashMap;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub token_expires_at: i64,
    pub started_at: i64,
    pub arrays: HashMap<String, Vec<String>>,
    pub tou_api_key: String,
    pub tou_utility_eia_id: u32,
    pub tou_rate_label: String,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health::get_health))
        .route("/api/energy/windows", get(energy::get_windows))
        .route("/api/energy/windows/latest", get(energy::get_latest))
        .route("/api/inverters/snapshots", get(inverters::get_snapshots))
        .route(
            "/api/inverters/snapshots/window/{window_start}",
            get(inverters::get_snapshots_by_window),
        )
        .route("/api/inverters/arrays", get(arrays::get_arrays))
        .route("/api/tou/refresh", post(tou::refresh_tou))
        .route("/api/trueup/estimate", get(trueup::get_estimate))
        .with_state(state)
}

pub async fn serve(state: AppState, host: &str, port: u16) -> anyhow::Result<()> {
    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(event = "api_server_start", addr = %addr);
    axum::serve(listener, create_router(state)).await?;
    Ok(())
}
