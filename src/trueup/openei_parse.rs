use super::calculator::PeriodRate;
use crate::error::{AppError, TouError};

/// Parses an `energyweekdayschedule` or `energyweekendschedule` JSON array into a
/// `Vec<Vec<usize>>` — one inner `Vec` per month, each element being a period index
/// for that hour (0-indexed).
pub(super) fn parse_schedule(val: &serde_json::Value) -> Result<Vec<Vec<usize>>, AppError> {
    let months = val.as_array().ok_or_else(|| {
        AppError::Tou(TouError::ParseError(
            "energyweekdayschedule is not an array".into(),
        ))
    })?;
    months
        .iter()
        .map(|month_val| {
            let hours = month_val.as_array().ok_or_else(|| {
                AppError::Tou(TouError::ParseError(
                    "schedule month is not an array".into(),
                ))
            })?;
            hours
                .iter()
                .map(|h| {
                    h.as_i64().map(|n| n as usize).ok_or_else(|| {
                        AppError::Tou(TouError::ParseError("invalid period index".into()))
                    })
                })
                .collect()
        })
        .collect()
}

/// Parses an `energyratestructure` JSON array into a `Vec<PeriodRate>` — one entry per
/// rate period.  If the `"sell"` key is absent from a tier, falls back to the buy rate and
/// emits a `tou_sell_rate_missing` warning.
pub(super) fn parse_period_rates(val: &serde_json::Value) -> Result<Vec<PeriodRate>, AppError> {
    let periods = val.as_array().ok_or_else(|| {
        AppError::Tou(TouError::ParseError(
            "energyratestructure is not an array".into(),
        ))
    })?;
    periods
        .iter()
        .enumerate()
        .map(|(i, period_val)| {
            let tiers = period_val.as_array().ok_or_else(|| {
                AppError::Tou(TouError::ParseError("rate period is not an array".into()))
            })?;
            let tier = tiers.first().ok_or_else(|| {
                AppError::Tou(TouError::ParseError("rate period has no tiers".into()))
            })?;
            let rate = tier["rate"].as_f64().ok_or_else(|| {
                AppError::Tou(TouError::ParseError(format!(
                    "missing 'rate' field in tier {i}"
                )))
            })?;
            let sell_rate = if let Some(s) = tier["sell"].as_f64() {
                s
            } else {
                tracing::warn!(event = "tou_sell_rate_missing", tier = i);
                rate
            };
            Ok(PeriodRate { rate, sell_rate })
        })
        .collect()
}
