use crate::api::server::AppState;
use crate::error::ApiError;
use crate::error::AppError;
use crate::storage::tou_schedule;
use crate::tou::history::{History, ScheduleMeta, TouInterval};
use crate::tou::openei_client::OpenEiClient;
use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct RefreshResponse {
    schedule_id: i64,
    rate_label: String,
    utility_name: String,
    effective_date: Option<String>,
    fetched_at: i64,
    revision_count: usize,
}

pub async fn refresh_tou(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let client = OpenEiClient::with_base_url(
        state.tou_api_key.clone(),
        state.tou_utility_eia_id,
        state.tou_rate_label.clone(),
        state.tou_openei_base_url.clone(),
    );
    let fetched = client.fetch_history().await?;

    let fetched_at = crate::util::unix_now();
    let ids =
        tou_schedule::insert_history(&state.pool, state.tou_utility_eia_id, fetched_at, &fetched)
            .await?;
    let schedule_id = *ids
        .last()
        .ok_or(AppError::Tou(crate::error::TouError::NoSchedule))?;
    let revision_count = fetched.len();
    let fetched = fetched
        .last()
        .ok_or(AppError::Tou(crate::error::TouError::NoSchedule))?;

    tracing::info!(
        event = "tou_refresh_complete",
        schedule_id,
        rate_label = %fetched.rate_label,
    );

    Ok(Json(RefreshResponse {
        schedule_id,
        rate_label: fetched.rate_label.clone(),
        utility_name: fetched.utility_name.clone(),
        effective_date: fetched.effective_date.clone(),
        fetched_at,
        revision_count,
    }))
}

#[derive(Deserialize)]
pub struct IntervalsQuery {
    start: Option<String>,
    end: Option<String>,
}

#[derive(Serialize)]
struct IntervalsResponse {
    timezone: String,
    schedules: Vec<ScheduleMeta>,
    intervals: Vec<TouInterval>,
}

pub async fn get_intervals(
    State(state): State<AppState>,
    Query(query): Query<IntervalsQuery>,
) -> Result<impl IntoResponse, AppError> {
    let parse = |value: Option<String>| -> Result<i64, AppError> {
        // Leave room for timezone offsets and neighboring dates at chrono's extremes.
        const CALENDAR_MARGIN: i64 = 2 * 24 * 3600;
        value
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|timestamp| {
                *timestamp > chrono::DateTime::<chrono::Utc>::MIN_UTC.timestamp() + CALENDAR_MARGIN
                    && *timestamp
                        < chrono::DateTime::<chrono::Utc>::MAX_UTC.timestamp() - CALENDAR_MARGIN
            })
            .ok_or_else(|| {
                AppError::Api(ApiError::InvalidParam(
                    "start and end must be valid Unix epoch integers".into(),
                ))
            })
    };
    let start = parse(query.start)?;
    let end = parse(query.end)?;
    const MAX_INTERVAL_RANGE: i64 = 31 * 24 * 3600;
    if end <= start
        || end
            .checked_sub(start)
            .is_none_or(|duration| duration > MAX_INTERVAL_RANGE)
    {
        return Err(AppError::Api(ApiError::InvalidParam(
            "range must be positive and at most 31 days".into(),
        )));
    }
    let history =
        History::load(&state.pool, state.tou_utility_eia_id, &state.tou_rate_label).await?;
    let (intervals, schedules) = history.intervals(start, end, state.tou_timezone)?;
    Ok(Json(IntervalsResponse {
        timezone: state.tou_timezone.name().to_string(),
        schedules,
        intervals,
    }))
}
