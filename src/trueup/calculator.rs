use crate::error::{AppError, TouError};
use crate::storage::models::{EnergyWindow, TouRateSchedule};
use chrono::{Datelike, TimeZone, Timelike};
use chrono_tz::America::Los_Angeles;
use std::collections::{BTreeSet, HashMap};

use super::openei_parse::{parse_period_rates, parse_schedule};

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

pub(crate) struct PeriodRate {
    pub(crate) rate: f64,
    pub(crate) sell_rate: f64,
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
    let month_maps =
        build_per_month_maps(schedule.id, &weekday_sched, &weekend_sched, &period_rates)?;

    tracing::info!(
        event = "trueup_calc_start",
        windows = %windows.len(),
        schedule_id = %schedule.id,
        period_count = %period_rates.len(),
    );

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

        let tou_period = month_maps[month].get(&period_idx).copied().ok_or_else(|| {
            AppError::Tou(TouError::ParseError(format!(
                "period_idx={period_idx} not in month={month} map"
            )))
        })?;

        let rates = &period_rates[period_idx]; // invariant: build_per_month_maps pre-validated this index

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

fn build_per_month_maps(
    schedule_id: i64,
    weekday_sched: &[Vec<usize>],
    weekend_sched: &[Vec<usize>],
    period_rates: &[PeriodRate],
) -> Result<[HashMap<usize, TouPeriod>; 12], AppError> {
    let mut maps: [HashMap<usize, TouPeriod>; 12] = std::array::from_fn(|_| HashMap::new());

    for (month, month_map_slot) in maps.iter_mut().enumerate() {
        let wd = weekday_sched.get(month).map(Vec::as_slice).unwrap_or(&[]);
        let we = weekend_sched.get(month).map(Vec::as_slice).unwrap_or(&[]);

        // Union of active period indices — BTreeSet gives sorted, deterministic iteration
        let active: BTreeSet<usize> = wd.iter().chain(we.iter()).copied().collect();
        let active_count = active.len();

        if active_count < 2 {
            tracing::warn!(
                event = "tou_month_degenerate",
                schedule_id = schedule_id,
                month = month,
                active_count = active_count,
                reason = if active_count == 0 {
                    "no_periods"
                } else {
                    "single_period"
                },
            );
        }

        // Validate all indices and collect (index, rate) pairs.
        // BTreeSet iteration is ascending by index, so equal-rate ties preserve lower-index order
        // after the stable sort below.
        let mut ranked: Vec<(usize, f64)> = active
            .into_iter()
            .map(|idx| {
                period_rates
                    .get(idx)
                    .map(|pr| (idx, pr.rate))
                    .ok_or_else(|| {
                        AppError::Tou(TouError::ParseError(format!(
                            "period_idx={idx} missing in energyratestructure for month={month}"
                        )))
                    })
            })
            .collect::<Result<Vec<_>, AppError>>()?;

        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let n = ranked.len();
        let peak_idx = ranked.first().map(|(i, _)| *i);
        let super_off_peak_idx = if n >= 3 {
            ranked.last().map(|(i, _)| *i)
        } else {
            None
        };

        let mut month_map = HashMap::new();
        for (rank, (idx, _)) in ranked.iter().enumerate() {
            let period = if rank == 0 {
                TouPeriod::Peak
            } else if n >= 3 && rank == n - 1 {
                TouPeriod::SuperOffPeak
            } else {
                TouPeriod::OffPeak
            };
            month_map.insert(*idx, period);
        }

        tracing::info!(
            event = "tou_period_map_built",
            schedule_id = schedule_id,
            month = month,
            active_count = active_count,
            peak_idx = ?peak_idx,
            super_off_peak_idx = ?super_off_peak_idx,
        );

        *month_map_slot = month_map;
    }

    Ok(maps)
}
