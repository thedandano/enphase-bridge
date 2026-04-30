use crate::api::handlers::energy::parse_iso_or;
use crate::api::server::AppState;
use crate::error::{ApiError, AppError, TouError};
use crate::storage::models::TrueUpEstimate;
use crate::storage::{energy_window, tou_schedule, true_up};
use crate::trueup::calculator;
use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct EstimateQuery {
    start: Option<String>,
    end: Option<String>,
}

#[derive(Serialize)]
pub struct EstimateResponse {
    period_start: i64,
    period_end: i64,
    net_cost_usd: f64,
    breakdown: Breakdown,
    tou_schedule: ScheduleMeta,
    computed_at: i64,
}

#[derive(Serialize)]
struct Breakdown {
    peak: PeriodDetail,
    off_peak: PeriodDetail,
    super_off_peak: PeriodDetail,
}

#[derive(Serialize)]
struct PeriodDetail {
    import_kwh: f64,
    export_kwh: f64,
    import_cost_usd: f64,
    export_credit_usd: f64,
}

#[derive(Serialize)]
struct ScheduleMeta {
    id: i64,
    rate_label: String,
    effective_date: Option<String>,
}

pub async fn get_estimate(
    State(state): State<AppState>,
    Query(params): Query<EstimateQuery>,
) -> Result<impl IntoResponse, AppError> {
    let start_str = params.start.as_deref().ok_or_else(|| {
        AppError::Api(ApiError::InvalidParam("start and end are required".into()))
    })?;
    let end_str = params.end.as_deref().ok_or_else(|| {
        AppError::Api(ApiError::InvalidParam("start and end are required".into()))
    })?;

    let period_start = parse_iso_or(Some(start_str), 0)?;
    let period_end = parse_iso_or(Some(end_str), 0)?;

    if period_end < period_start {
        return Err(AppError::Api(ApiError::InvalidParam(
            "end must be after start".into(),
        )));
    }

    // energy_window::query_range uses exclusive end (`window_start < ?`); add one day so the
    // user-supplied UTC midnight date is inclusive. Callers must pass `end` as a UTC instant.
    let period_end = period_end + 86_400;

    let schedule = tou_schedule::query_latest(&state.pool, &state.tou_rate_label)
        .await?
        .ok_or(AppError::Tou(TouError::NoSchedule))?;

    let windows =
        energy_window::query_range(&state.pool, period_start, period_end, 50_000, 0).await?;

    if windows.is_empty() {
        return Err(AppError::Api(ApiError::InsufficientData(
            "no energy windows found for the requested period".into(),
        )));
    }

    let result = calculator::calculate(&schedule, &windows)?;

    let computed_at = crate::util::unix_now();

    let estimate = TrueUpEstimate {
        id: 0,
        computed_at,
        period_start,
        period_end,
        net_cost_usd: result.net_cost_usd,
        peak_import_kwh: result.peak.import_kwh,
        peak_export_kwh: result.peak.export_kwh,
        offpeak_import_kwh: result.off_peak.import_kwh,
        offpeak_export_kwh: result.off_peak.export_kwh,
        super_offpeak_import_kwh: result.super_off_peak.import_kwh,
        super_offpeak_export_kwh: result.super_off_peak.export_kwh,
        tou_schedule_id: schedule.id,
    };
    if let Err(e) = true_up::insert(&state.pool, &estimate).await {
        tracing::error!(event = "trueup_persist_failed", error = %e);
    }

    Ok(Json(EstimateResponse {
        period_start,
        period_end,
        net_cost_usd: round2(result.net_cost_usd),
        breakdown: Breakdown {
            peak: PeriodDetail {
                import_kwh: round3(result.peak.import_kwh),
                export_kwh: round3(result.peak.export_kwh),
                import_cost_usd: round2(result.peak.import_cost_usd),
                export_credit_usd: round2(result.peak.export_credit_usd),
            },
            off_peak: PeriodDetail {
                import_kwh: round3(result.off_peak.import_kwh),
                export_kwh: round3(result.off_peak.export_kwh),
                import_cost_usd: round2(result.off_peak.import_cost_usd),
                export_credit_usd: round2(result.off_peak.export_credit_usd),
            },
            super_off_peak: PeriodDetail {
                import_kwh: round3(result.super_off_peak.import_kwh),
                export_kwh: round3(result.super_off_peak.export_kwh),
                import_cost_usd: round2(result.super_off_peak.import_cost_usd),
                export_credit_usd: round2(result.super_off_peak.export_credit_usd),
            },
        },
        tou_schedule: ScheduleMeta {
            id: schedule.id,
            rate_label: schedule.rate_label,
            effective_date: schedule.effective_date,
        },
        computed_at,
    }))
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}
