use crate::error::{AppError, TouError};
use crate::storage::models::{EnergyWindow, TouRateSchedule};
use chrono::{Datelike, TimeZone, Timelike};
use chrono_tz::America::Los_Angeles;
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct PeriodSummary {
    pub import_kwh: f64,
    pub export_kwh: f64,
    pub import_cost_usd: f64,
    pub export_credit_usd: f64,
}

#[derive(Debug, Clone)]
pub struct CalculatorResult {
    pub peak: PeriodSummary,
    pub off_peak: PeriodSummary,
    pub super_off_peak: PeriodSummary,
    pub net_cost_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TouPeriod {
    Peak,
    OffPeak,
    SuperOffPeak,
}

struct PeriodRate {
    rate: f64,
    sell_rate: f64,
}

pub fn calculate(
    schedule: &TouRateSchedule,
    windows: &[EnergyWindow],
) -> Result<CalculatorResult, AppError> {
    let rate_json: serde_json::Value = serde_json::from_str(&schedule.rate_json)
        .map_err(|e| AppError::Tou(TouError::ParseError(e.to_string())))?;

    let weekday_sched = parse_schedule(&rate_json["energyweekdayschedule"])?;
    let weekend_sched = parse_schedule(&rate_json["energyweekendschedule"])?;
    let period_rates = parse_period_rates(&rate_json["energyratestructure"])?;
    let period_map = build_period_map(&period_rates);

    tracing::info!(
        event = "trueup_calc_start",
        windows = %windows.len(),
        schedule_id = %schedule.id,
        period_count = %period_rates.len(),
    );

    tracing::debug!(event = "tou_period_map", period_map = ?period_map);

    let mut peak = PeriodSummary::default();
    let mut off_peak = PeriodSummary::default();
    let mut super_off_peak = PeriodSummary::default();

    for window in windows {
        let local_dt = chrono::Utc
            .timestamp_opt(window.window_start, 0)
            .single()
            .ok_or_else(|| {
                AppError::Tou(TouError::ParseError(format!(
                    "invalid window timestamp: {}",
                    window.window_start
                )))
            })?
            .with_timezone(&Los_Angeles);

        let month = local_dt.month0() as usize;
        let hour = local_dt.hour() as usize;
        let is_weekend = matches!(
            local_dt.weekday(),
            chrono::Weekday::Sat | chrono::Weekday::Sun
        );

        let sched = if is_weekend {
            &weekend_sched
        } else {
            &weekday_sched
        };
        let period_idx = sched
            .get(month)
            .and_then(|m| m.get(hour))
            .copied()
            .ok_or_else(|| {
                AppError::Tou(TouError::ParseError(format!(
                    "schedule out of bounds: month={month}, hour={hour}"
                )))
            })?;

        let rates = period_rates.get(period_idx).ok_or_else(|| {
            AppError::Tou(TouError::ParseError(format!(
                "period index {period_idx} out of range"
            )))
        })?;

        let tou_period = period_map
            .get(&period_idx)
            .copied()
            .unwrap_or(TouPeriod::OffPeak);

        let import_kwh = window.wh_grid_import / 1000.0;
        let export_kwh = window.wh_grid_export / 1000.0;

        let acc = match tou_period {
            TouPeriod::Peak => &mut peak,
            TouPeriod::OffPeak => &mut off_peak,
            TouPeriod::SuperOffPeak => &mut super_off_peak,
        };
        acc.import_kwh += import_kwh;
        acc.export_kwh += export_kwh;
        acc.import_cost_usd += import_kwh * rates.rate;
        acc.export_credit_usd += export_kwh * rates.sell_rate;
    }

    let net_cost_usd = (peak.import_cost_usd
        + off_peak.import_cost_usd
        + super_off_peak.import_cost_usd)
        - (peak.export_credit_usd + off_peak.export_credit_usd + super_off_peak.export_credit_usd);

    tracing::info!(
        event = "trueup_calc_done",
        peak_kwh = peak.import_kwh,
        offpeak_kwh = off_peak.import_kwh,
        super_offpeak_kwh = super_off_peak.import_kwh,
        net_cost_usd = net_cost_usd,
    );

    Ok(CalculatorResult {
        peak,
        off_peak,
        super_off_peak,
        net_cost_usd,
    })
}

fn parse_schedule(val: &serde_json::Value) -> Result<Vec<Vec<usize>>, AppError> {
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

fn parse_period_rates(val: &serde_json::Value) -> Result<Vec<PeriodRate>, AppError> {
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

fn build_period_map(period_rates: &[PeriodRate]) -> HashMap<usize, TouPeriod> {
    let mut ranked: Vec<(usize, f64)> = period_rates
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.rate))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let n = ranked.len();
    if !(2..=4).contains(&n) {
        tracing::warn!(event = "tou_period_count_unexpected", n = %n);
    }

    let mut map = HashMap::new();
    for (rank, (idx, _)) in ranked.iter().enumerate() {
        let period = if rank == 0 {
            TouPeriod::Peak
        } else if n >= 3 && rank == n - 1 {
            TouPeriod::SuperOffPeak
        } else {
            TouPeriod::OffPeak
        };
        map.insert(*idx, period);
    }
    map
}
