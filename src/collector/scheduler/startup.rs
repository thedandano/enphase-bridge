use sqlx::SqlitePool;
use tracing::{error, info};

use crate::collector::window_aggregator::{
    CURRENT_FORMULA_VERSION, CumulativeReading, compute_delta,
};
use crate::storage::{boundary_snapshot, config_store, energy_window as ew_store};

use super::{KEY_GRID_EXPORT_WH, KEY_GRID_IMPORT_WH, KEY_LAST_TS, KEY_PROD_WH};

/// Load the persisted cumulative reading from the config_store table.
/// Returns None on cold start (no prior state) or if any required key is missing.
pub(super) async fn load_persisted_reading(pool: &SqlitePool) -> Option<CumulativeReading> {
    let ts = config_store::get(pool, KEY_LAST_TS).await.ok()??;
    let prod = config_store::get(pool, KEY_PROD_WH).await.ok()??;
    let grid_import = config_store::get(pool, KEY_GRID_IMPORT_WH).await.ok()??;
    let grid_export = config_store::get(pool, KEY_GRID_EXPORT_WH).await.ok()??;
    Some(CumulativeReading {
        timestamp: ts.parse().ok()?,
        production_wh: prod.parse().ok()?,
        grid_import_cum_wh: grid_import.parse().ok()?,
        grid_export_cum_wh: grid_export.parse().ok()?,
    })
}

/// Recompute stale energy windows using stored boundary snapshot pairs.
/// Called once at scheduler startup after session auth and meter probe.
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
