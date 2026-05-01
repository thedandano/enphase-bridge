use crate::api::server::AppState;
use crate::error::{ApiError, AppError};
use crate::storage::power_sample;
use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SamplesQuery {
    start: Option<i64>,
    end: Option<i64>,
    limit: Option<i32>,
    offset: Option<i32>,
}

#[derive(Serialize)]
pub struct SamplesResponse {
    samples: Vec<SampleItem>,
    total: usize,
    limit: i32,
    offset: i32,
}

#[derive(Serialize)]
pub struct SampleItem {
    pub sampled_at: i64,
    pub production_w: f64,
    pub consumption_w: f64,
    pub grid_w: f64,
}

pub async fn get_power_samples(
    State(state): State<AppState>,
    Query(params): Query<SamplesQuery>,
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

    let rows = power_sample::query_range(&state.pool, start, end, limit, offset).await?;
    let total = rows.len();
    let samples = rows
        .into_iter()
        .map(|r| SampleItem {
            sampled_at: r.sampled_at,
            production_w: r.production_w,
            consumption_w: r.consumption_w,
            grid_w: r.grid_w,
        })
        .collect();

    Ok(Json(SamplesResponse {
        samples,
        total,
        limit,
        offset,
    }))
}
