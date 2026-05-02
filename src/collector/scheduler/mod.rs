mod boundary;
mod startup;
mod window_close;

use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

use crate::collector::gateway_client::GatewayClient;
use crate::collector::window_aggregator::{CumulativeReading, window_boundary};
use crate::constants::DAY_SECS;
use crate::storage::{phase_reading as phase_store, power_sample as ps_store};

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

        startup::startup_recompute(&self.pool).await;

        let mut ticker = time::interval(self.interval);
        let mut last_reading = startup::load_persisted_reading(&self.pool).await;
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

        self.process_window_boundary(last_reading, curr, now, &readings, accumulator)
            .await;

        run_retention_if_due(
            &self.pool,
            now,
            last_retention_check,
            self.retention_days,
            self.phase_retention_days,
        )
        .await;
    }

    /// Decide whether `now` crosses a 15-min window boundary; on crossing, run window-close and
    /// reset the accumulator. On a mid-window tick, the previous anchor is kept frozen. On cold
    /// start (no previous reading), `curr` is stored as the new anchor.
    async fn process_window_boundary(
        &self,
        last_reading: &mut Option<CumulativeReading>,
        curr: CumulativeReading,
        now: i64,
        readings: &crate::collector::gateway_client::MeterReadings,
        accumulator: &mut Vec<(f64, f64, f64)>,
    ) {
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
                        readings,
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

// Re-export for integration test callers that import via `collector::scheduler::startup_recompute`.
#[allow(unused_imports)]
pub use startup::startup_recompute;
