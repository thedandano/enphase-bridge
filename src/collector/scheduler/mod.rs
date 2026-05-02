mod boundary;
mod window_close;

use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

use crate::collector::gateway_client::GatewayClient;
use crate::collector::window_aggregator::{
    CURRENT_FORMULA_VERSION, CumulativeReading, compute_delta, window_boundary,
};
use crate::constants::DAY_SECS;
use crate::storage::{
    boundary_snapshot, config_store, energy_window as ew_store, phase_reading as phase_store,
    power_sample as ps_store,
};

const KEY_LAST_TS: &str = "last_poll_timestamp";
const KEY_PROD_WH: &str = "last_cumulative_production_wh";
const KEY_GRID_IMPORT_WH: &str = "last_cumulative_grid_import_wh";
const KEY_GRID_EXPORT_WH: &str = "last_cumulative_grid_export_wh";

pub struct Scheduler {
    gateway: GatewayClient,
    pool: SqlitePool,
    interval: Duration,
    retention_days: u32,
    phase_retention_days: u32,
}

impl Scheduler {
    pub fn new(
        gateway: GatewayClient,
        pool: SqlitePool,
        interval_secs: u64,
        retention_days: u32,
        phase_retention_days: u32,
    ) -> Self {
        Self {
            gateway,
            pool,
            interval: Duration::from_secs(interval_secs),
            retention_days,
            phase_retention_days,
        }
    }

    pub async fn run(mut self) {
        if let Err(e) = self.gateway.check_jwt().await {
            error!(event = "session_auth_failed", error = %e, message = "cannot acquire gateway session; scheduler halted");
            return;
        }

        if let Err(e) = self.gateway.probe_meters().await {
            error!(event = "meter_probe_failed", error = %e, message = "required meter absent or unreachable; scheduler halted");
            return;
        }

        startup_recompute(&self.pool).await;

        let mut ticker = time::interval(self.interval);
        let mut last_reading = self.load_persisted_reading().await;
        let mut accumulator: Vec<(f64, f64, f64)> = Vec::new();
        let mut last_retention_check: i64 = 0;

        info!(
            event = "scheduler_start",
            interval_secs = self.interval.as_secs(),
            has_prior_state = last_reading.is_some()
        );

        loop {
            ticker.tick().await;
            self.poll_tick(
                &mut last_reading,
                &mut accumulator,
                &mut last_retention_check,
            )
            .await;
        }
    }

    /// Execute one poll tick: fetch readings, persist power sample, detect window boundary,
    /// and run data-retention pruning when a day has elapsed.
    async fn poll_tick(
        &mut self,
        last_reading: &mut Option<CumulativeReading>,
        accumulator: &mut Vec<(f64, f64, f64)>,
        last_retention_check: &mut i64,
    ) {
        let readings = match self.gateway.get_meter_readings().await {
            Ok(r) => r,
            Err(e) => {
                error!(event = "poll_error", error = %e);
                return;
            }
        };

        let now = crate::util::unix_now();
        let curr = CumulativeReading {
            timestamp: now,
            production_wh: readings.production_cum_wh,
            grid_import_cum_wh: readings.grid_import_cum_wh,
            grid_export_cum_wh: readings.grid_export_cum_wh,
        };

        // Per-tick: insert power sample
        boundary::handle_power_sample(
            &self.pool,
            now,
            readings.production_w_now,
            readings.consumption_w_now,
            readings.grid_w_now,
        )
        .await;
        accumulator.push((
            readings.production_w_now,
            readings.consumption_w_now,
            readings.grid_w_now,
        ));

        if let Some(prev) = last_reading.take() {
            let prev_window = window_boundary(prev.timestamp);
            let curr_window = window_boundary(now);

            if curr_window > prev_window {
                window_close::handle_window_close(
                    &self.pool,
                    &self.gateway,
                    &window_close::WindowCloseArgs {
                        prev_window,
                        prev: &prev,
                        curr: &curr,
                        now,
                        readings: &readings,
                        accumulator: accumulator.as_slice(),
                    },
                )
                .await;
                accumulator.clear();
                *last_reading = Some(curr);
            } else {
                // Mid-window tick: put prev back; anchor stays frozen at the previous boundary reading.
                *last_reading = Some(prev);
            }
        } else {
            *last_reading = Some(curr);
        }

        run_retention_if_due(
            &self.pool,
            now,
            last_retention_check,
            self.retention_days,
            self.phase_retention_days,
        )
        .await;
    }

    async fn load_persisted_reading(&self) -> Option<CumulativeReading> {
        let ts = config_store::get(&self.pool, KEY_LAST_TS).await.ok()??;
        let prod = config_store::get(&self.pool, KEY_PROD_WH).await.ok()??;
        let grid_import = config_store::get(&self.pool, KEY_GRID_IMPORT_WH)
            .await
            .ok()??;
        let grid_export = config_store::get(&self.pool, KEY_GRID_EXPORT_WH)
            .await
            .ok()??;
        Some(CumulativeReading {
            timestamp: ts.parse().ok()?,
            production_wh: prod.parse().ok()?,
            grid_import_cum_wh: grid_import.parse().ok()?,
            grid_export_cum_wh: grid_export.parse().ok()?,
        })
    }
}

/// Prune power-sample and phase-reading rows older than the configured retention windows,
/// but only once per day to avoid repeated I/O.
async fn run_retention_if_due(
    pool: &SqlitePool,
    now: i64,
    last_retention_check: &mut i64,
    retention_days: u32,
    phase_retention_days: u32,
) {
    if now - *last_retention_check < DAY_SECS {
        return;
    }
    let cutoff = now - (retention_days as i64 * DAY_SECS);
    match ps_store::delete_before(pool, cutoff).await {
        Ok(deleted) => info!(event = "power_sample_retention", deleted, cutoff),
        Err(e) => error!(event = "power_sample_retention_error", error = %e),
    }
    let phase_cutoff = now - (phase_retention_days as i64 * DAY_SECS);
    match phase_store::delete_before(pool, phase_cutoff).await {
        Ok(deleted) => info!(
            event = "phase_reading_retention",
            deleted,
            cutoff = phase_cutoff
        ),
        Err(e) => error!(event = "phase_reading_retention_error", error = %e),
    }
    *last_retention_check = now;
}

// Recompute stale energy windows using stored boundary snapshot pairs.
pub async fn startup_recompute(pool: &SqlitePool) {
    let stale = match ew_store::query_stale(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            error!(event = "startup_recompute_query_failed", error = %e);
            return;
        }
    };

    let (mut examined, mut updated, mut no_anchor) = (0usize, 0usize, 0usize);

    for row in &stale {
        examined += 1;
        match boundary_snapshot::query_pair(pool, row.window_start).await {
            Ok(None) => {
                tracing::warn!(
                    event = "recompute_no_anchor",
                    window_start = row.window_start
                );
                no_anchor += 1;
            }
            Ok(Some((prev, curr))) => {
                let prev_reading = CumulativeReading {
                    timestamp: prev.captured_at,
                    production_wh: prev.production_wh,
                    grid_import_cum_wh: prev.grid_import_cum_wh,
                    grid_export_cum_wh: prev.grid_export_cum_wh,
                };
                let curr_reading = CumulativeReading {
                    timestamp: curr.captured_at,
                    production_wh: curr.production_wh,
                    grid_import_cum_wh: curr.grid_import_cum_wh,
                    grid_export_cum_wh: curr.grid_export_cum_wh,
                };
                let delta = compute_delta(row.window_start, &prev_reading, &curr_reading, true);
                if delta.was_clamped {
                    tracing::warn!(
                        event = "energy_balance_clamped",
                        window_start = delta.window_start
                    );
                }
                match ew_store::update_recomputed(
                    pool,
                    row.window_start,
                    delta.wh_produced,
                    delta.wh_consumed,
                    delta.wh_grid_import,
                    delta.wh_grid_export,
                    CURRENT_FORMULA_VERSION,
                    delta.was_clamped,
                )
                .await
                {
                    Ok(()) => updated += 1,
                    Err(e) => {
                        error!(
                            event = "recompute_update_failed",
                            window_start = row.window_start,
                            error = %e
                        )
                    }
                }
            }
            Err(e) => {
                error!(
                    event = "recompute_pair_query_failed",
                    window_start = row.window_start,
                    error = %e
                );
            }
        }
    }

    info!(
        event = "startup_recompute_complete",
        examined, updated, no_anchor
    );
}
