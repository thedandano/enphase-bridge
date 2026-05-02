// These functions are called exclusively by the `recompute_windows` binary.
// The lib target has no direct callers, so dead_code lint must be suppressed here.
#![allow(dead_code)]

use crate::collector::gateway_client::extract_cumulatives_from_json;
use crate::collector::window_aggregator::{
    CURRENT_FORMULA_VERSION, CumulativeReading, compute_delta,
};
use crate::storage::energy_window::FormulaFilter;
use crate::storage::{boundary_snapshot, energy_window};
use sqlx::SqlitePool;
use tracing::{error, info, warn};

/// Recomputes all stale `energy_window` rows (formula_version < current) from typed
/// `boundary_snapshot` columns.
pub async fn run_typed(pool: &SqlitePool, from: Option<i64>, to: Option<i64>, dry_run: bool) {
    let stale = match energy_window::query_stale(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            error!("query_stale failed: {}", e);
            return;
        }
    };

    let filtered: Vec<_> = stale
        .iter()
        .filter(|row| {
            from.map(|f| row.window_start >= f).unwrap_or(true)
                && to.map(|t| row.window_start < t).unwrap_or(true)
        })
        .collect();

    let (mut examined, mut updated, mut skipped_no_anchor) = (0usize, 0usize, 0usize);

    for row in &filtered {
        examined += 1;
        match boundary_snapshot::query_pair(pool, row.window_start).await {
            Ok(None) => {
                warn!(
                    event = "recompute_no_anchor",
                    window_start = row.window_start
                );
                skipped_no_anchor += 1;
            }
            Ok(Some((prev, curr))) => {
                let prev_r = CumulativeReading {
                    timestamp: prev.captured_at,
                    production_wh: prev.production_wh,
                    grid_import_cum_wh: prev.grid_import_cum_wh,
                    grid_export_cum_wh: prev.grid_export_cum_wh,
                };
                let curr_r = CumulativeReading {
                    timestamp: curr.captured_at,
                    production_wh: curr.production_wh,
                    grid_import_cum_wh: curr.grid_import_cum_wh,
                    grid_export_cum_wh: curr.grid_export_cum_wh,
                };
                if apply_recompute(pool, row.window_start, prev_r, curr_r, dry_run).await {
                    updated += 1;
                }
            }
            Err(e) => error!(
                event = "query_pair_failed",
                window_start = row.window_start,
                error = %e
            ),
        }
    }

    info!(
        event = "recompute_typed_complete",
        examined, updated, skipped_no_anchor, dry_run
    );
}

/// Recomputes `energy_window` rows from raw `boundary_snapshot.raw_meters_json` blobs.
pub async fn run_raw(pool: &SqlitePool, from: Option<i64>, to: Option<i64>, dry_run: bool) {
    let rows = match energy_window::query_range(
        pool,
        from.unwrap_or(0),
        to.unwrap_or(i64::MAX),
        i32::MAX,
        0,
        FormulaFilter::Recomputable,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("query_range failed: {}", e);
            return;
        }
    };

    let (mut examined, mut updated, mut skipped_no_anchor, skipped_unrecomputable) =
        (0usize, 0usize, 0usize, 0usize);

    for row in &rows {
        examined += 1;
        match boundary_snapshot::query_pair(pool, row.window_start).await {
            Ok(None) => {
                warn!(
                    event = "recompute_no_anchor",
                    window_start = row.window_start
                );
                skipped_no_anchor += 1;
            }
            Ok(Some((prev_snap, curr_snap))) => {
                let prev_readings = match extract_cumulatives_from_json(&prev_snap.raw_meters_json)
                {
                    Ok(r) => r,
                    Err(e) => {
                        error!(
                            event = "raw_parse_failed",
                            window_start = row.window_start,
                            snapshot = "prev",
                            error = %e
                        );
                        continue;
                    }
                };
                let curr_readings = match extract_cumulatives_from_json(&curr_snap.raw_meters_json)
                {
                    Ok(r) => r,
                    Err(e) => {
                        error!(
                            event = "raw_parse_failed",
                            window_start = row.window_start,
                            snapshot = "curr",
                            error = %e
                        );
                        continue;
                    }
                };
                let prev_r = CumulativeReading {
                    timestamp: prev_snap.captured_at,
                    production_wh: prev_readings.production_cum_wh,
                    grid_import_cum_wh: prev_readings.grid_import_cum_wh,
                    grid_export_cum_wh: prev_readings.grid_export_cum_wh,
                };
                let curr_r = CumulativeReading {
                    timestamp: curr_snap.captured_at,
                    production_wh: curr_readings.production_cum_wh,
                    grid_import_cum_wh: curr_readings.grid_import_cum_wh,
                    grid_export_cum_wh: curr_readings.grid_export_cum_wh,
                };
                if apply_recompute(pool, row.window_start, prev_r, curr_r, dry_run).await {
                    updated += 1;
                }
            }
            Err(e) => error!(
                event = "query_pair_failed",
                window_start = row.window_start,
                error = %e
            ),
        }
    }

    info!(
        event = "recompute_raw_complete",
        examined, updated, skipped_no_anchor, skipped_unrecomputable, dry_run
    );
}

/// Shared inner helper: computes the delta from a prev/curr pair and either logs a dry-run
/// preview or writes the updated values to the database.  Returns `true` if the row was
/// updated (or would have been updated in dry-run mode).
async fn apply_recompute(
    pool: &SqlitePool,
    window_start: i64,
    prev_r: CumulativeReading,
    curr_r: CumulativeReading,
    dry_run: bool,
) -> bool {
    let delta = compute_delta(window_start, &prev_r, &curr_r, true);
    if dry_run {
        info!(
            event = "dry_run_would_update",
            window_start,
            wh_produced = delta.wh_produced,
            wh_consumed = delta.wh_consumed,
            wh_grid_import = delta.wh_grid_import,
            wh_grid_export = delta.wh_grid_export,
            new_formula_version = CURRENT_FORMULA_VERSION,
        );
        return true;
    }
    match energy_window::update_recomputed(
        pool,
        window_start,
        delta.wh_produced,
        delta.wh_consumed,
        delta.wh_grid_import,
        delta.wh_grid_export,
        CURRENT_FORMULA_VERSION,
        delta.was_clamped,
    )
    .await
    {
        Ok(()) => true,
        Err(e) => {
            error!(
                event = "update_failed",
                window_start,
                error = %e
            );
            false
        }
    }
}
