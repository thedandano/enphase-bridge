use crate::api::server::AppState;
use crate::error::AppError;
use crate::storage::{config_store, energy_window, tou_schedule};
use axum::{Json, extract::State, response::IntoResponse};
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    last_window_start: Option<i64>,
    token_expires_at: i64,
    uptime_seconds: i64,
    tou_schedule_id: Option<i64>,
    tou_fetched_at: Option<i64>,
    tou_stale: bool,
    unrecomputable_window_count: Option<i64>,
    stale_window_count: Option<i64>,
    clamped_window_count: Option<i64>,
}

pub async fn get_health(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let last_window_start = config_store::get(&state.pool, "last_window_start")
        .await?
        .and_then(|s| s.parse::<i64>().ok());

    let now = crate::util::unix_now();
    let uptime_seconds = now - state.started_at;

    let tou = tou_schedule::query_latest(&state.pool, &state.tou_rate_label).await?;
    let (tou_schedule_id, tou_fetched_at, tou_stale) = match tou {
        None => (None, None, true),
        Some(s) => {
            let stale = (now - s.fetched_at) > 90 * 24 * 3600;
            (Some(s.id), Some(s.fetched_at), stale)
        }
    };

    let unrecomputable_window_count = match energy_window::count_unrecomputable(&state.pool).await {
        Ok(n) => Some(n),
        Err(e) => {
            tracing::error!(event = "health_count_unrecomputable_failed", error = %e);
            None
        }
    };
    let stale_window_count = match energy_window::count_stale(&state.pool).await {
        Ok(n) => Some(n),
        Err(e) => {
            tracing::error!(event = "health_count_stale_failed", error = %e);
            None
        }
    };
    let clamped_window_count = match energy_window::count_clamped(&state.pool).await {
        Ok(n) => Some(n),
        Err(e) => {
            tracing::error!(event = "health_count_clamped_failed", error = %e);
            None
        }
    };

    Ok(Json(HealthResponse {
        status: "ok",
        last_window_start,
        token_expires_at: state.token_expires_at,
        uptime_seconds,
        tou_schedule_id,
        tou_fetched_at,
        tou_stale,
        unrecomputable_window_count,
        stale_window_count,
        clamped_window_count,
    }))
}
