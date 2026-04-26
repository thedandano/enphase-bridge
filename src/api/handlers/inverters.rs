use crate::api::handlers::energy::parse_iso_or;
use crate::api::server::AppState;
use crate::error::{ApiError, AppError};
use crate::storage::inverter_snapshot;
use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SnapshotsQuery {
    start: Option<String>,
    end: Option<String>,
    serial: Option<String>,
    limit: Option<i32>,
    offset: Option<i32>,
}

#[derive(Serialize)]
pub struct SnapshotsResponse {
    snapshots: Vec<SnapshotItem>,
    total: usize,
    limit: i32,
    offset: i32,
}

#[derive(Serialize)]
struct SnapshotItem {
    window_start: i64,
    serial_number: String,
    watts_output: f64,
    is_online: bool,
}

#[derive(Serialize)]
pub struct WindowInvertersResponse {
    pub window_start: i64,
    pub inverters: Vec<InverterItem>,
}

#[derive(Serialize)]
pub struct InverterItem {
    pub serial_number: String,
    pub watts_output: f64,
    pub is_online: bool,
}

pub async fn get_snapshots(
    State(state): State<AppState>,
    Query(params): Query<SnapshotsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let now = unix_now();
    let start = parse_iso_or(params.start.as_deref(), now - 86400)?;
    let end = parse_iso_or(params.end.as_deref(), now)?;
    let limit = params.limit.unwrap_or(200).clamp(1, 2000);
    let offset = params.offset.unwrap_or(0).max(0);

    let rows = match params.serial.as_deref() {
        Some(serial) => {
            inverter_snapshot::query_by_serial_range(&state.pool, serial, start, end, limit, offset)
                .await?
        }
        None => inverter_snapshot::query_range(&state.pool, start, end, limit, offset).await?,
    };

    let total = rows.len();
    let snapshots = rows
        .into_iter()
        .map(|s| SnapshotItem {
            window_start: s.window_start,
            serial_number: s.serial_number,
            watts_output: s.watts_output,
            is_online: s.is_online,
        })
        .collect();

    Ok(Json(SnapshotsResponse {
        snapshots,
        total,
        limit,
        offset,
    }))
}

pub async fn get_snapshots_by_window(
    State(state): State<AppState>,
    Path(window_start): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let rows = inverter_snapshot::query_by_window(&state.pool, window_start).await?;

    if rows.is_empty() {
        return Err(AppError::Api(ApiError::NotFound(format!(
            "no inverter snapshots for window {window_start}"
        ))));
    }

    let inverters = rows
        .into_iter()
        .map(|s| InverterItem {
            serial_number: s.serial_number,
            watts_output: s.watts_output,
            is_online: s.is_online,
        })
        .collect();

    Ok(Json(WindowInvertersResponse {
        window_start,
        inverters,
    }))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
