use sqlx::{Sqlite, SqlitePool, Transaction};
use tracing::{error, info, warn};

use crate::collector::window_aggregator::{CumulativeReading, compute_delta};
use crate::storage::{config_store, energy_window as ew_store, power_sample as ps_store};

/// Averaged watts over the accumulator ticks for a closed window.
pub(super) struct WindowAverages {
    pub prod: Option<f64>,
    pub cons: Option<f64>,
    pub grid: Option<f64>,
}

/// Context for a window boundary transaction.
pub(super) struct BoundaryCtx<'a> {
    pub prev_window: i64,
    pub prev: &'a CumulativeReading,
    pub curr: &'a CumulativeReading,
    pub now: i64,
    pub raw_json: &'a str,
    pub avgs: &'a WindowAverages,
}

/// Insert one power-sample row for the current poll tick.
/// Errors are logged and swallowed — a missing sample does not halt the poll cycle.
pub(super) async fn handle_power_sample(
    pool: &SqlitePool,
    now: i64,
    production_w: f64,
    consumption_w: f64,
    grid_w: f64,
) {
    if let Err(e) = ps_store::insert(pool, now, production_w, consumption_w, grid_w).await {
        error!(event = "power_sample_insert_error", sampled_at = now, error = %e);
    }
}

/// Run the boundary_snapshot + energy_window transaction for a window crossing.
///
/// Handles three branches:
///  - Normal path: new snapshot inserted + energy_window not yet present → compute delta, write window.
///  - AlreadyExists + window present: duplicate crossing (restart) → log and rollback.
///  - AlreadyExists + window absent: repair path (crash between snapshot and window write) → write window.
///
/// JSON too-large path (> 262 144 bytes) skips the snapshot and writes a formula_version=0 window.
///
/// Errors are logged internally; the caller continues regardless of outcome.
pub(super) async fn handle_boundary_snapshot_tx(pool: &SqlitePool, ctx: &BoundaryCtx<'_>) {
    let raw_json_bytes = ctx.raw_json.len();

    if raw_json_bytes > 262_144 {
        // JSON too large — skip snapshot, write window as unrecomputable (formula_version = 0)
        error!(
            event = "boundary_json_too_large",
            bytes = raw_json_bytes,
            window_start = ctx.prev_window
        );
        let mut unrecomputable_window = compute_delta(ctx.prev_window, ctx.prev, ctx.curr, true);
        unrecomputable_window.formula_version = 0;
        unrecomputable_window.avg_production_w = ctx.avgs.prod;
        unrecomputable_window.avg_consumption_w = ctx.avgs.cons;
        unrecomputable_window.avg_grid_w = ctx.avgs.grid;
        if unrecomputable_window.was_clamped {
            warn!(
                event = "energy_balance_clamped",
                window_start = unrecomputable_window.window_start
            );
        }
        match ew_store::insert(pool, &unrecomputable_window).await {
            Ok(()) => {
                info!(
                    event = "window_stored_unrecomputable",
                    window_start = ctx.prev_window,
                    reason = "boundary_json_too_large"
                );
                let _ = config_store::set(pool, "last_window_start", &ctx.prev_window.to_string())
                    .await;
            }
            Err(e) => error!(event = "window_store_error", error = %e),
        }
        return;
    }

    // Warn if large but within limit
    if raw_json_bytes > 32_768 {
        warn!(
            event = "boundary_json_large",
            bytes = raw_json_bytes,
            window_start = ctx.prev_window
        );
    }

    // Transaction covering boundary_snapshot + energy_window
    let tx = match pool.begin().await {
        Err(e) => {
            error!(event = "boundary_tx_begin_failed", error = %e);
            return;
        }
        Ok(tx) => tx,
    };

    run_boundary_tx(tx, pool, ctx).await;
}

async fn run_boundary_tx(
    mut tx: Transaction<'_, Sqlite>,
    pool: &SqlitePool,
    ctx: &BoundaryCtx<'_>,
) {
    // Insert boundary snapshot (INSERT OR IGNORE on unique window_start)
    let snap_result = sqlx::query(
        "INSERT OR IGNORE INTO boundary_snapshot
         (window_start, production_wh, grid_import_cum_wh, grid_export_cum_wh, captured_at, raw_meters_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(ctx.prev_window)
    .bind(ctx.curr.production_wh)
    .bind(ctx.curr.grid_import_cum_wh)
    .bind(ctx.curr.grid_export_cum_wh)
    .bind(ctx.now)
    .bind(ctx.raw_json)
    .execute(&mut *tx)
    .await;

    match snap_result {
        Err(e) => {
            let _ = tx.rollback().await;
            error!(
                event = "boundary_snapshot_insert_failed",
                window_start = ctx.prev_window,
                error = %e
            );
        }
        Ok(snap_r) => {
            let snapshot_inserted = snap_r.rows_affected() == 1;

            // Check whether energy_window already exists for this boundary
            match sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM energy_window WHERE window_start = ?)",
            )
            .bind(ctx.prev_window)
            .fetch_one(&mut *tx)
            .await
            {
                Err(e) => {
                    let _ = tx.rollback().await;
                    error!(event = "ew_exists_check_failed", window_start = ctx.prev_window, error = %e);
                    // skip window write; fall through to inverter snapshots + persist_reading
                }
                Ok(ew_exists) => {
                    if snapshot_inserted {
                        // Normal path: new snapshot + new window
                        insert_window_in_tx(tx, pool, ctx, false).await;
                    } else if ew_exists {
                        // AlreadyExists + window present: duplicate boundary crossing (restart)
                        let _ = tx.rollback().await;
                        info!(
                            event = "boundary_already_complete",
                            window_start = ctx.prev_window
                        );
                    } else {
                        // AlreadyExists + window absent: repair path
                        insert_window_in_tx(tx, pool, ctx, true).await;
                    }
                }
            }
        }
    }
}

/// Insert or repair an energy_window row inside the open transaction, then commit.
/// `is_repair` distinguishes the event name logged on success.
async fn insert_window_in_tx(
    mut tx: Transaction<'_, Sqlite>,
    pool: &SqlitePool,
    ctx: &BoundaryCtx<'_>,
    is_repair: bool,
) {
    let window = compute_delta(ctx.prev_window, ctx.prev, ctx.curr, true);
    if window.was_clamped {
        warn!(
            event = "energy_balance_clamped",
            window_start = window.window_start
        );
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
    .bind(ctx.avgs.prod)
    .bind(ctx.avgs.cons)
    .bind(ctx.avgs.grid)
    .execute(&mut *tx)
    .await;

    match insert_result {
        Err(e) => {
            let _ = tx.rollback().await;
            if is_repair {
                error!(event = "window_repair_store_error", error = %e);
            } else {
                error!(event = "window_store_error", error = %e);
            }
        }
        Ok(_) => {
            if let Err(e) = tx.commit().await {
                error!(event = "boundary_tx_commit_failed", error = %e);
            } else if is_repair {
                info!(event = "window_repaired", window_start = ctx.prev_window);
                let _ = config_store::set(pool, "last_window_start", &ctx.prev_window.to_string())
                    .await;
            } else {
                info!(
                    event = "window_stored",
                    window_start = ctx.prev_window,
                    wh_produced = window.wh_produced,
                    wh_consumed = window.wh_consumed,
                    wh_grid_export = window.wh_grid_export,
                    wh_grid_import = window.wh_grid_import,
                );
                let _ = config_store::set(pool, "last_window_start", &ctx.prev_window.to_string())
                    .await;
            }
        }
    }
}
