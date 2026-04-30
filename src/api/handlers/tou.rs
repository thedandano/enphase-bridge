use crate::api::server::AppState;
use crate::error::AppError;
use crate::storage::tou_schedule;
use crate::tou::openei_client::OpenEiClient;
use axum::{Json, extract::State, response::IntoResponse};
use serde::Serialize;

#[derive(Serialize)]
struct RefreshResponse {
    schedule_id: i64,
    rate_label: String,
    utility_name: String,
    effective_date: Option<String>,
    fetched_at: i64,
}

pub async fn refresh_tou(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let client = OpenEiClient::with_base_url(
        state.tou_api_key.clone(),
        state.tou_utility_eia_id,
        state.tou_rate_label.clone(),
        state.tou_openei_base_url.clone(),
    );
    let fetched = client.fetch().await?;

    let fetched_at = crate::util::unix_now();
    let schedule_id = tou_schedule::insert(
        &state.pool,
        fetched_at,
        fetched.effective_date.as_deref(),
        &fetched.utility_name,
        &fetched.rate_label,
        &fetched.rate_json,
    )
    .await?;

    tracing::info!(
        event = "tou_refresh_complete",
        schedule_id,
        rate_label = %fetched.rate_label,
    );

    Ok(Json(RefreshResponse {
        schedule_id,
        rate_label: fetched.rate_label,
        utility_name: fetched.utility_name,
        effective_date: fetched.effective_date,
        fetched_at,
    }))
}
