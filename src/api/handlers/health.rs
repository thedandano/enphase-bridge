use crate::api::server::AppState;
use crate::error::AppError;
use crate::storage::config_store;
use axum::{Json, extract::State, response::IntoResponse};
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    last_window_start: Option<i64>,
    token_expires_at: i64,
    uptime_seconds: i64,
}

pub async fn get_health(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let last_window_start = config_store::get(&state.pool, "last_window_start")
        .await?
        .and_then(|s| s.parse::<i64>().ok());

    let uptime_seconds = unix_now() - state.started_at;

    Ok(Json(HealthResponse {
        status: "ok",
        last_window_start,
        token_expires_at: state.token_expires_at,
        uptime_seconds,
    }))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
