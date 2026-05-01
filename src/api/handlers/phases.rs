use crate::api::server::AppState;
use crate::error::{ApiError, AppError};
use crate::storage::phase_reading;
use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PhaseReadingsQuery {
    start: Option<i64>,
    end: Option<i64>,
    meter_eid: Option<i64>,
    limit: Option<i32>,
    offset: Option<i32>,
}

#[derive(Serialize)]
pub struct PhaseReadingsResponse {
    readings: Vec<PhaseReadingItem>,
    total: usize,
    limit: i32,
    offset: i32,
}

#[derive(Serialize)]
pub struct PhaseReadingItem {
    pub sampled_at: i64,
    pub meter_eid: i64,
    pub channel_eid: i64,
    pub active_power_w_at_boundary: f64,
    pub energy_dlvd_wh: f64,
    pub energy_rcvd_wh: f64,
}

pub async fn get_phase_readings(
    State(state): State<AppState>,
    Query(params): Query<PhaseReadingsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let start = params
        .start
        .ok_or_else(|| AppError::Api(ApiError::InvalidParam("start is required".into())))?;
    let end = params
        .end
        .ok_or_else(|| AppError::Api(ApiError::InvalidParam("end is required".into())))?;

    if end < start {
        return Err(AppError::Api(ApiError::InvalidParam(
            "end must be after start".into(),
        )));
    }

    let limit = params.limit.unwrap_or(500).clamp(1, 5000);
    let offset = params.offset.unwrap_or(0).max(0);

    let rows = phase_reading::query_range(&state.pool, start, end, params.meter_eid, limit, offset)
        .await?;
    let total = rows.len();
    let readings = rows
        .into_iter()
        .map(|r| PhaseReadingItem {
            sampled_at: r.sampled_at,
            meter_eid: r.meter_eid,
            channel_eid: r.channel_eid,
            active_power_w_at_boundary: r.active_power_w_at_boundary,
            energy_dlvd_wh: r.energy_dlvd_wh,
            energy_rcvd_wh: r.energy_rcvd_wh,
        })
        .collect();

    Ok(Json(PhaseReadingsResponse {
        readings,
        total,
        limit,
        offset,
    }))
}
