use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time;
use tracing::{error, info, warn};

use crate::collector::gateway_client::GatewayClient;
use crate::collector::window_aggregator::{
    CURRENT_FORMULA_VERSION, CumulativeReading, compute_delta, window_boundary,
};
use crate::storage::{
    boundary_snapshot, config_store, energy_window as ew_store, inverter_snapshot as inv_store,
    phase_reading as phase_store, power_sample as ps_store,
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

        // Task 8.1: recompute stale windows using stored boundary snapshots before entering the poll loop
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

            let readings = match self.gateway.get_meter_readings().await {
                Ok(r) => r,
                Err(e) => {
                    error!(event = "poll_error", error = %e);
                    continue;
                }
            };

            let now = crate::util::unix_now();
            let curr = CumulativeReading {
                timestamp: now,
                production_wh: readings.production_cum_wh,
                grid_import_cum_wh: readings.grid_import_cum_wh,
                grid_export_cum_wh: readings.grid_export_cum_wh,
            };

            if let Err(e) = ps_store::insert(
                &self.pool,
                now,
                readings.production_w_now,
                readings.consumption_w_now,
                readings.grid_w_now,
            )
            .await
            {
                error!(event = "power_sample_insert_error", sampled_at = now, error = %e);
            }
            accumulator.push((
                readings.production_w_now,
                readings.consumption_w_now,
                readings.grid_w_now,
            ));

            if let Some(prev) = &last_reading {
                let prev_window = window_boundary(prev.timestamp);
                let curr_window = window_boundary(now);

                if curr_window > prev_window {
                    let (avg_prod, avg_cons, avg_grid) = if accumulator.is_empty() {
                        (None, None, None)
                    } else {
                        let n = accumulator.len() as f64;
                        (
                            Some(accumulator.iter().map(|s| s.0).sum::<f64>() / n),
                            Some(accumulator.iter().map(|s| s.1).sum::<f64>() / n),
                            Some(accumulator.iter().map(|s| s.2).sum::<f64>() / n),
                        )
                    };
                    accumulator.clear();

                    // Tasks 6.2 / 6.3: JSON size checks before deciding path
                    let raw_json_bytes = readings.raw_json.len();

                    if raw_json_bytes > 262_144 {
                        // JSON too large — skip snapshot, write window as unrecomputable (formula_version = 0)
                        error!(
                            event = "boundary_json_too_large",
                            bytes = raw_json_bytes,
                            window_start = prev_window
                        );
                        let mut unrecomputable_window =
                            compute_delta(prev_window, prev, &curr, true);
                        unrecomputable_window.formula_version = 0;
                        unrecomputable_window.avg_production_w = avg_prod;
                        unrecomputable_window.avg_consumption_w = avg_cons;
                        unrecomputable_window.avg_grid_w = avg_grid;
                        if unrecomputable_window.was_clamped {
                            warn!(
                                event = "energy_balance_clamped",
                                window_start = unrecomputable_window.window_start
                            );
                        }
                        match ew_store::insert(&self.pool, &unrecomputable_window).await {
                            Ok(()) => {
                                info!(
                                    event = "window_stored_unrecomputable",
                                    window_start = prev_window,
                                    reason = "boundary_json_too_large"
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
                    } else {
                        // Task 6.3: warn if large but within limit
                        if raw_json_bytes > 32_768 {
                            warn!(
                                event = "boundary_json_large",
                                bytes = raw_json_bytes,
                                window_start = prev_window
                            );
                        }

                        // Task 6.1: transaction covering boundary_snapshot + energy_window
                        match self.pool.begin().await {
                            Err(e) => {
                                error!(event = "boundary_tx_begin_failed", error = %e);
                            }
                            Ok(mut tx) => {
                                // Insert boundary snapshot (INSERT OR IGNORE on unique window_start)
                                let snap_result = sqlx::query(
                                    "INSERT OR IGNORE INTO boundary_snapshot
                                     (window_start, production_wh, grid_import_cum_wh, grid_export_cum_wh, captured_at, raw_meters_json)
                                     VALUES (?, ?, ?, ?, ?, ?)",
                                )
                                .bind(prev_window)
                                .bind(curr.production_wh)
                                .bind(curr.grid_import_cum_wh)
                                .bind(curr.grid_export_cum_wh)
                                .bind(now)
                                .bind(&readings.raw_json)
                                .execute(&mut *tx)
                                .await;

                                match snap_result {
                                    Err(e) => {
                                        let _ = tx.rollback().await;
                                        error!(
                                            event = "boundary_snapshot_insert_failed",
                                            window_start = prev_window,
                                            error = %e
                                        );
                                    }
                                    Ok(snap_r) => {
                                        let snapshot_inserted = snap_r.rows_affected() == 1;

                                        // Check whether energy_window already exists for this boundary
                                        match sqlx::query_scalar::<_, bool>(
                                            "SELECT EXISTS(SELECT 1 FROM energy_window WHERE window_start = ?)",
                                        )
                                        .bind(prev_window)
                                        .fetch_one(&mut *tx)
                                        .await
                                        {
                                            Err(e) => {
                                                let _ = tx.rollback().await;
                                                error!(event = "ew_exists_check_failed", window_start = prev_window, error = %e);
                                                // skip window write; fall through to inverter snapshots + persist_reading
                                            }
                                            Ok(ew_exists) => {
                                                if snapshot_inserted {
                                                    // Normal path: new snapshot + new window
                                                    let window =
                                                        compute_delta(prev_window, prev, &curr, true);
                                                    if window.was_clamped {
                                                        warn!(event = "energy_balance_clamped", window_start = window.window_start);
                                                    }
                                                    let insert_result = sqlx::query(
                                                        "INSERT OR IGNORE INTO energy_window
                                                         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped, avg_production_w, avg_consumption_w, avg_grid_w)
                                                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                                                    )
                                                    .bind(window.window_start)
                                                    .bind(window.wh_produced)
                                                    .bind(window.wh_consumed)
                                                    .bind(window.wh_grid_import)
                                                    .bind(window.wh_grid_export)
                                                    .bind(window.is_complete)
                                                    .bind(window.formula_version)
                                                    .bind(window.was_clamped)
                                                    .bind(avg_prod)
                                                    .bind(avg_cons)
                                                    .bind(avg_grid)
                                                    .execute(&mut *tx)
                                                    .await;

                                                    match insert_result {
                                                        Err(e) => {
                                                            let _ = tx.rollback().await;
                                                            error!(event = "window_store_error", error = %e);
                                                        }
                                                        Ok(_) => {
                                                            if let Err(e) = tx.commit().await {
                                                                error!(event = "boundary_tx_commit_failed", error = %e);
                                                            } else {
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
                                                        }
                                                    }
                                                } else if ew_exists {
                                                    // AlreadyExists + window present: duplicate boundary crossing (restart)
                                                    let _ = tx.rollback().await;
                                                    info!(event = "boundary_already_complete", window_start = prev_window);
                                                } else {
                                                    // AlreadyExists + window absent: repair path (crash between snapshot and window write)
                                                    let window =
                                                        compute_delta(prev_window, prev, &curr, true);
                                                    if window.was_clamped {
                                                        warn!(event = "energy_balance_clamped", window_start = window.window_start);
                                                    }
                                                    let insert_result = sqlx::query(
                                                        "INSERT OR IGNORE INTO energy_window
                                                         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped, avg_production_w, avg_consumption_w, avg_grid_w)
                                                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                                                    )
                                                    .bind(window.window_start)
                                                    .bind(window.wh_produced)
                                                    .bind(window.wh_consumed)
                                                    .bind(window.wh_grid_import)
                                                    .bind(window.wh_grid_export)
                                                    .bind(window.is_complete)
                                                    .bind(window.formula_version)
                                                    .bind(window.was_clamped)
                                                    .bind(avg_prod)
                                                    .bind(avg_cons)
                                                    .bind(avg_grid)
                                                    .execute(&mut *tx)
                                                    .await;

                                                    match insert_result {
                                                        Err(e) => {
                                                            let _ = tx.rollback().await;
                                                            error!(event = "window_repair_store_error", error = %e);
                                                        }
                                                        Ok(_) => {
                                                            if let Err(e) = tx.commit().await {
                                                                error!(event = "boundary_tx_commit_failed", error = %e);
                                                            } else {
                                                                info!(event = "window_repaired", window_start = prev_window);
                                                                let _ = config_store::set(
                                                                    &self.pool,
                                                                    "last_window_start",
                                                                    &prev_window.to_string(),
                                                                )
                                                                .await;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Inverter snapshots — always attempt regardless of boundary outcome
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

                    // Phase readings at boundary crossing — fire-and-forget; failure does not block poll cycle
                    if !readings.channel_readings.is_empty() {
                        let phase_rows: Vec<crate::storage::models::PhaseReading> = readings
                            .channel_readings
                            .iter()
                            .map(|ch| crate::storage::models::PhaseReading {
                                id: 0,
                                sampled_at: now,
                                meter_eid: ch.meter_eid as i64,
                                channel_eid: ch.channel_eid as i64,
                                active_power_w_at_boundary: ch.active_power,
                                energy_dlvd_wh: ch.act_energy_dlvd,
                                energy_rcvd_wh: ch.act_energy_rcvd,
                            })
                            .collect();
                        if let Err(e) = phase_store::insert_batch(&self.pool, &phase_rows).await {
                            error!(event = "phase_reading_insert_error", sampled_at = now, error = %e);
                        } else {
                            info!(
                                event = "phase_readings_stored",
                                count = phase_rows.len(),
                                sampled_at = now
                            );
                        }
                    }

                    self.persist_reading(&curr).await;
                    last_reading = Some(curr);
                }
                // Mid-window tick: anchor stays frozen at the previous boundary reading.
            } else {
                last_reading = Some(curr);
            }

            if now - last_retention_check >= 86400 {
                let cutoff = now - (self.retention_days as i64 * 86400);
                match ps_store::delete_before(&self.pool, cutoff).await {
                    Ok(deleted) => info!(event = "power_sample_retention", deleted, cutoff),
                    Err(e) => error!(event = "power_sample_retention_error", error = %e),
                }
                let phase_cutoff = now - (self.phase_retention_days as i64 * 86400);
                match phase_store::delete_before(&self.pool, phase_cutoff).await {
                    Ok(deleted) => info!(
                        event = "phase_reading_retention",
                        deleted,
                        cutoff = phase_cutoff
                    ),
                    Err(e) => error!(event = "phase_reading_retention_error", error = %e),
                }
                last_retention_check = now;
            }
        }
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

    async fn persist_reading(&self, r: &CumulativeReading) {
        let _ = config_store::set(&self.pool, KEY_LAST_TS, &r.timestamp.to_string()).await;
        let _ = config_store::set(&self.pool, KEY_PROD_WH, &r.production_wh.to_string()).await;
        let _ = config_store::set(
            &self.pool,
            KEY_GRID_IMPORT_WH,
            &r.grid_import_cum_wh.to_string(),
        )
        .await;
        let _ = config_store::set(
            &self.pool,
            KEY_GRID_EXPORT_WH,
            &r.grid_export_cum_wh.to_string(),
        )
        .await;
    }
}

// Task 8.1: recompute stale energy windows using stored boundary snapshot pairs.
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
                warn!(
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
                    warn!(
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
