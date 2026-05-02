use sqlx::SqlitePool;
use tracing::{error, info};

use crate::collector::gateway_client::{ChannelReading, GatewayClient, MeterReadings};
use crate::collector::scheduler::boundary::{
    BoundaryCtx, WindowAverages, handle_boundary_snapshot_tx,
};
use crate::collector::window_aggregator::CumulativeReading;
use crate::storage::models::PhaseReading;
use crate::storage::{config_store, inverter_snapshot as inv_store, phase_reading as phase_store};

/// Arguments for a window-close operation.
pub(super) struct WindowCloseArgs<'a> {
    pub prev_window: i64,
    pub prev: &'a CumulativeReading,
    pub curr: &'a CumulativeReading,
    pub now: i64,
    pub readings: &'a MeterReadings,
    pub accumulator: &'a [(f64, f64, f64)],
}

/// Handle a window boundary crossing: compute averages, run the boundary_snapshot + energy_window
/// transaction, persist inverter snapshots and phase readings, then update the persisted reading.
///
/// Side-effect ordering (must match characterization test):
///  1. Compute averages from accumulator
///  2. boundary_snapshot + energy_window transaction (via `boundary::handle_boundary_snapshot_tx`)
///  3. Inverter snapshots — always, regardless of transaction outcome
///  4. Phase readings — fire-and-forget
///  5. config_store persist_reading (× 4 keys)
pub(super) async fn handle_window_close(
    pool: &SqlitePool,
    gateway: &GatewayClient,
    args: &WindowCloseArgs<'_>,
) {
    let WindowCloseArgs {
        prev_window,
        prev,
        curr,
        now,
        readings,
        accumulator,
    } = args;
    let (prev_window, now) = (*prev_window, *now);

    // Step 1: compute averages from the accumulator
    let avgs = compute_averages(accumulator);

    // Step 2: boundary_snapshot + energy_window transaction
    let ctx = BoundaryCtx {
        prev_window,
        prev,
        curr,
        now,
        raw_json: &readings.raw_json,
        avgs: &avgs,
    };
    handle_boundary_snapshot_tx(pool, &ctx).await;

    // Step 3: inverter snapshots — always attempt regardless of boundary outcome
    match gateway.get_inverter_snapshots(prev_window).await {
        Ok(snapshots) => {
            let count = snapshots.len();
            match inv_store::insert_batch(pool, &snapshots).await {
                Ok(()) => info!(
                    event = "inverter_snapshots_stored",
                    count,
                    window_start = prev_window,
                ),
                Err(e) => {
                    error!(event = "inverter_snapshot_store_error", error = %e)
                }
            }
        }
        Err(e) => error!(event = "inverter_snapshot_fetch_error", error = %e),
    }

    // Step 4: phase readings — fire-and-forget; failure does not block poll cycle
    handle_phase_readings(pool, now, &readings.channel_readings).await;

    // Step 5: persist cumulative reading (config_store)
    persist_reading(pool, curr).await;
}

fn compute_averages(accumulator: &[(f64, f64, f64)]) -> WindowAverages {
    if accumulator.is_empty() {
        return WindowAverages {
            prod: None,
            cons: None,
            grid: None,
        };
    }
    let n = accumulator.len() as f64;
    WindowAverages {
        prod: Some(accumulator.iter().map(|s| s.0).sum::<f64>() / n),
        cons: Some(accumulator.iter().map(|s| s.1).sum::<f64>() / n),
        grid: Some(accumulator.iter().map(|s| s.2).sum::<f64>() / n),
    }
}

async fn handle_phase_readings(pool: &SqlitePool, now: i64, channel_readings: &[ChannelReading]) {
    if channel_readings.is_empty() {
        return;
    }
    let phase_rows: Vec<PhaseReading> = channel_readings
        .iter()
        .map(|ch| PhaseReading {
            id: 0,
            sampled_at: now,
            meter_eid: ch.meter_eid as i64,
            channel_eid: ch.channel_eid as i64,
            active_power_w_at_boundary: ch.active_power,
            energy_dlvd_wh: ch.act_energy_dlvd,
            energy_rcvd_wh: ch.act_energy_rcvd,
        })
        .collect();
    if let Err(e) = phase_store::insert_batch(pool, &phase_rows).await {
        error!(event = "phase_reading_insert_error", sampled_at = now, error = %e);
    } else {
        info!(
            event = "phase_readings_stored",
            count = phase_rows.len(),
            sampled_at = now
        );
    }
}

async fn persist_reading(pool: &SqlitePool, r: &CumulativeReading) {
    let _ = config_store::set(pool, "last_poll_timestamp", &r.timestamp.to_string()).await;
    let _ = config_store::set(
        pool,
        "last_cumulative_production_wh",
        &r.production_wh.to_string(),
    )
    .await;
    let _ = config_store::set(
        pool,
        "last_cumulative_grid_import_wh",
        &r.grid_import_cum_wh.to_string(),
    )
    .await;
    let _ = config_store::set(
        pool,
        "last_cumulative_grid_export_wh",
        &r.grid_export_cum_wh.to_string(),
    )
    .await;
}
