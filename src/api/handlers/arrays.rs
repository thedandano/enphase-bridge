use crate::api::server::AppState;
use crate::error::AppError;
use crate::storage::inverter_snapshot;
use axum::{Json, extract::State, response::IntoResponse};
use serde::Serialize;

#[derive(Serialize)]
pub struct ArraysResponse {
    window_start: Option<i64>,
    arrays: Vec<ArraySummary>,
}

#[derive(Serialize)]
struct ArraySummary {
    name: String,
    total_watts: f64,
    online_count: usize,
    total_count: usize,
    inverters: Vec<InverterEntry>,
}

#[derive(Serialize)]
struct InverterEntry {
    serial_number: String,
    watts_output: f64,
    is_online: bool,
}

pub async fn get_arrays(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let latest = inverter_snapshot::query_latest_window(&state.pool).await?;

    let (window_start, snapshots) = match latest {
        None => {
            return Ok(Json(ArraysResponse {
                window_start: None,
                arrays: vec![],
            }));
        }
        Some((ws, rows)) => (ws, rows),
    };

    // Build serial → snapshot lookup
    let by_serial: std::collections::HashMap<&str, _> = snapshots
        .iter()
        .map(|s| (s.serial_number.as_str(), s))
        .collect();

    let mut arrays: Vec<ArraySummary> = state
        .arrays
        .iter()
        .map(|(name, serials)| {
            let inverters: Vec<InverterEntry> = serials
                .iter()
                .map(|sn| match by_serial.get(sn.as_str()) {
                    Some(s) => InverterEntry {
                        serial_number: sn.clone(),
                        watts_output: s.watts_output,
                        is_online: s.is_online,
                    },
                    None => InverterEntry {
                        serial_number: sn.clone(),
                        watts_output: 0.0,
                        is_online: false,
                    },
                })
                .collect();

            let total_watts: f64 = inverters.iter().map(|i| i.watts_output).sum();
            let online_count = inverters.iter().filter(|i| i.is_online).count();
            let total_count = inverters.len();

            ArraySummary {
                name: name.clone(),
                total_watts,
                online_count,
                total_count,
                inverters,
            }
        })
        .collect();

    arrays.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(ArraysResponse {
        window_start: Some(window_start),
        arrays,
    }))
}
