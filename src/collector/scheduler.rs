use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

use crate::collector::gateway_client::GatewayClient;
use crate::collector::window_aggregator::{CumulativeReading, compute_delta, window_boundary};
use crate::storage::{config_store, energy_window as ew_store, inverter_snapshot as inv_store};

const KEY_LAST_TS: &str = "last_poll_timestamp";
const KEY_PROD_WH: &str = "last_cumulative_production_wh";
const KEY_CONS_WH: &str = "last_cumulative_consumption_wh";

pub struct Scheduler {
    gateway: GatewayClient,
    pool: SqlitePool,
    interval: Duration,
}

impl Scheduler {
    pub fn new(gateway: GatewayClient, pool: SqlitePool, interval_secs: u64) -> Self {
        Self {
            gateway,
            pool,
            interval: Duration::from_secs(interval_secs),
        }
    }

    pub async fn run(mut self) {
        if let Err(e) = self.gateway.check_jwt().await {
            error!(event = "session_auth_failed", error = %e, message = "cannot acquire gateway session; scheduler halted");
            return;
        }

        let mut ticker = time::interval(self.interval);
        let mut last_reading = self.load_persisted_reading().await;

        info!(
            event = "scheduler_start",
            interval_secs = self.interval.as_secs(),
            has_prior_state = last_reading.is_some()
        );

        loop {
            ticker.tick().await;

            let readings = match self.gateway.get_meter_readings().await {
                Ok(r) => r,
                Err(e) => {
                    error!(event = "poll_error", error = %e);
                    continue;
                }
            };

            let now = unix_now();
            let curr = CumulativeReading {
                timestamp: now,
                production_wh: readings.production_cum_wh,
                consumption_wh: readings.consumption_cum_wh,
            };

            if let Some(prev) = &last_reading {
                let prev_window = window_boundary(prev.timestamp);
                let curr_window = window_boundary(now);

                if curr_window > prev_window {
                    let window = compute_delta(prev_window, prev, &curr, true);

                    match ew_store::insert(&self.pool, &window).await {
                        Ok(()) => {
                            info!(
                                event = "window_stored",
                                window_start = prev_window,
                                wh_produced = window.wh_produced,
                                wh_consumed = window.wh_consumed,
                                wh_grid_export = window.wh_grid_export,
                                wh_grid_import = window.wh_grid_import,
                            );
                            let _ = config_store::set(
                                &self.pool,
                                "last_window_start",
                                &prev_window.to_string(),
                            )
                            .await;
                        }
                        Err(e) => error!(event = "window_store_error", error = %e),
                    }

                    match self.gateway.get_inverter_snapshots(prev_window).await {
                        Ok(snapshots) => {
                            let count = snapshots.len();
                            match inv_store::insert_batch(&self.pool, &snapshots).await {
                                Ok(()) => info!(
                                    event = "inverter_snapshots_stored",
                                    count,
                                    window_start = prev_window
                                ),
                                Err(e) => {
                                    error!(event = "inverter_snapshot_store_error", error = %e)
                                }
                            }
                        }
                        Err(e) => error!(event = "inverter_snapshot_fetch_error", error = %e),
                    }

                    self.persist_reading(&curr).await;
                }
            }

            last_reading = Some(curr);
        }
    }

    async fn load_persisted_reading(&self) -> Option<CumulativeReading> {
        let ts = config_store::get(&self.pool, KEY_LAST_TS).await.ok()??;
        let prod = config_store::get(&self.pool, KEY_PROD_WH).await.ok()??;
        let cons = config_store::get(&self.pool, KEY_CONS_WH).await.ok()??;
        Some(CumulativeReading {
            timestamp: ts.parse().ok()?,
            production_wh: prod.parse().ok()?,
            consumption_wh: cons.parse().ok()?,
        })
    }

    async fn persist_reading(&self, r: &CumulativeReading) {
        let _ = config_store::set(&self.pool, KEY_LAST_TS, &r.timestamp.to_string()).await;
        let _ = config_store::set(&self.pool, KEY_PROD_WH, &r.production_wh.to_string()).await;
        let _ = config_store::set(&self.pool, KEY_CONS_WH, &r.consumption_wh.to_string()).await;
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
