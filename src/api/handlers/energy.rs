use crate::api::server::AppState;
use crate::error::{ApiError, AppError};
use crate::storage::energy_window;
use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct WindowsQuery {
    start: Option<String>,
    end: Option<String>,
    limit: Option<i32>,
    offset: Option<i32>,
}

#[derive(Serialize)]
pub struct WindowsResponse {
    windows: Vec<WindowItem>,
    total: usize,
    limit: i32,
    offset: i32,
}

#[derive(Serialize)]
pub struct WindowItem {
    pub window_start: i64,
    pub wh_produced: f64,
    pub wh_consumed: f64,
    pub wh_grid_import: f64,
    pub wh_grid_export: f64,
    pub is_complete: bool,
}

pub async fn get_windows(
    State(state): State<AppState>,
    Query(params): Query<WindowsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let now = unix_now();
    let start = parse_iso_or(params.start.as_deref(), now - 86400)?;
    let end = parse_iso_or(params.end.as_deref(), now)?;

    if start >= end {
        return Err(AppError::Api(ApiError::InvalidParam(
            "start must be before end".into(),
        )));
    }

    let limit = params.limit.unwrap_or(100).clamp(1, 2880);
    let offset = params.offset.unwrap_or(0).max(0);

    let rows = energy_window::query_range(&state.pool, start, end, limit, offset).await?;
    let total = rows.len();
    let windows = rows.into_iter().map(to_item).collect();

    Ok(Json(WindowsResponse {
        windows,
        total,
        limit,
        offset,
    }))
}

pub async fn get_latest(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    match energy_window::query_latest(&state.pool).await? {
        None => Err(AppError::Api(ApiError::NotFound(
            "no windows recorded yet".into(),
        ))),
        Some(w) => Ok(Json(to_item(w))),
    }
}

fn to_item(w: crate::storage::models::EnergyWindow) -> WindowItem {
    WindowItem {
        window_start: w.window_start,
        wh_produced: w.wh_produced,
        wh_consumed: w.wh_consumed,
        wh_grid_import: w.wh_grid_import,
        wh_grid_export: w.wh_grid_export,
        is_complete: w.is_complete,
    }
}

pub(crate) fn parse_iso_or(s: Option<&str>, default: i64) -> Result<i64, AppError> {
    match s {
        None => Ok(default),
        Some(raw) => DateTime::parse_from_rfc3339(raw)
            .map(|dt| dt.timestamp())
            .map_err(|_| AppError::Api(ApiError::InvalidParam(format!("invalid datetime: {raw}")))),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
