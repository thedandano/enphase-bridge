use clap::{Parser, ValueEnum};
use enphase_bridge::collector::gateway_client::extract_cumulatives_from_json;
use enphase_bridge::collector::window_aggregator::{
    CURRENT_FORMULA_VERSION, CumulativeReading, compute_delta,
};
use enphase_bridge::storage::energy_window::FormulaFilter;
use enphase_bridge::storage::{boundary_snapshot, energy_window};
use sqlx::SqlitePool;
use tracing::{error, info, warn};

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    Typed,
    Raw,
}

#[derive(Debug, Parser)]
#[command(about = "Manually recompute energy_window wh_* fields from stored boundary_snapshots")]
struct Args {
    #[arg(long, value_enum, default_value = "typed")]
    mode: Mode,
    #[arg(long)]
    from: Option<i64>,
    #[arg(long)]
    to: Option<i64>,
    #[arg(long, default_value = "false")]
    dry_run: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let config = enphase_bridge::config::Config::load().expect("failed to load config");

    let pool = enphase_bridge::storage::db::connect(&config.storage.db_path)
        .await
        .expect("failed to open database");

    match args.mode {
        Mode::Typed => run_typed(&pool, args.from, args.to, args.dry_run).await,
        Mode::Raw => run_raw(&pool, args.from, args.to, args.dry_run).await,
    }

    pool.close().await;
}

async fn run_typed(pool: &SqlitePool, from: Option<i64>, to: Option<i64>, dry_run: bool) {
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
                let delta = compute_delta(row.window_start, &prev_r, &curr_r, true);
                if dry_run {
                    info!(
                        event = "dry_run_would_update",
                        window_start = row.window_start,
                        wh_produced = delta.wh_produced,
                        wh_consumed = delta.wh_consumed,
                        wh_grid_import = delta.wh_grid_import,
                        wh_grid_export = delta.wh_grid_export,
                        new_formula_version = CURRENT_FORMULA_VERSION,
                    );
                    updated += 1;
                } else {
                    match energy_window::update_recomputed(
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
                        Err(e) => error!(
                            event = "update_failed",
                            window_start = row.window_start,
                            error = %e
                        ),
                    }
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

async fn run_raw(pool: &SqlitePool, from: Option<i64>, to: Option<i64>, dry_run: bool) {
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
                let delta = compute_delta(row.window_start, &prev_r, &curr_r, true);
                if dry_run {
                    info!(
                        event = "dry_run_would_update",
                        window_start = row.window_start,
                        wh_produced = delta.wh_produced,
                        wh_consumed = delta.wh_consumed,
                        wh_grid_import = delta.wh_grid_import,
                        wh_grid_export = delta.wh_grid_export,
                        new_formula_version = CURRENT_FORMULA_VERSION,
                    );
                    updated += 1;
                } else {
                    match energy_window::update_recomputed(
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
                        Err(e) => error!(
                            event = "update_failed",
                            window_start = row.window_start,
                            error = %e
                        ),
                    }
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
