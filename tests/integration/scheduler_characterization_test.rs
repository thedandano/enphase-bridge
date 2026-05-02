/// Characterization test: one complete window boundary crossing produces exactly the expected
/// row counts across all five affected tables and updates the documented config_store keys.
///
/// This test does NOT call `Scheduler::run()` (which requires a live gateway). Instead it
/// exercises the same storage calls in the same order that the scheduler's poll loop body
/// performs at a boundary crossing, using the real in-memory SQLite pool. The ordering here
/// is the authoritative reference for the M2 scheduler split: any refactor must preserve it.
///
/// Side-effect ordering documented here (matches scheduler.rs at time of M2 characterization):
///  1. ps_store::insert           — power_sample, every tick
///  2. accumulator.push           — per-tick (in-memory only; not stored)
///  3. [boundary crossed] boundary_snapshot+energy_window transaction
///  4. inv_store::insert_batch    — microinverter_snapshot
///  5. phase_store::insert_batch  — phase_reading
///  6. config_store::set × 4      — persist cumulative reading
use enphase_bridge::collector::window_aggregator::{CumulativeReading, compute_delta};
use enphase_bridge::storage::energy_window::FormulaFilter;
use enphase_bridge::storage::models::{MicroinverterSnapshot, PhaseReading};
use enphase_bridge::storage::{
    boundary_snapshot, config_store, energy_window as ew_store, inverter_snapshot as inv_store,
    phase_reading as phase_store, power_sample as ps_store,
};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

async fn test_pool() -> SqlitePool {
    let opts = SqliteConnectOptions::new()
        .filename(":memory:")
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("in-memory pool");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    pool
}

// 2024-01-01 00:15:00 UTC — the boundary that gets closed
const PREV_BOUNDARY: i64 = 1704067200; // 2024-01-01 00:00:00 UTC
const CURR_BOUNDARY: i64 = PREV_BOUNDARY + 900; // 2024-01-01 00:15:00 UTC
const TICK_TS: i64 = CURR_BOUNDARY + 5; // first tick after boundary

/// One boundary crossing must produce:
///   1 energy_window row
///   1 boundary_snapshot row
///   1 power_sample row  (the tick that crossed)
///   2 microinverter_snapshot rows (N inverters)
///   2 phase_reading rows  (M channels)
///   config_store keys: last_poll_timestamp, last_cumulative_production_wh,
///                      last_cumulative_grid_import_wh, last_cumulative_grid_export_wh,
///                      last_window_start
#[tokio::test]
async fn test_one_boundary_crossing_row_counts_and_config_store_keys() {
    let pool = test_pool().await;
    let now = TICK_TS;

    let prev = CumulativeReading {
        timestamp: PREV_BOUNDARY,
        production_wh: 100_000.0,
        grid_import_cum_wh: 5_000.0,
        grid_export_cum_wh: 1_000.0,
    };
    let curr = CumulativeReading {
        timestamp: now,
        production_wh: 100_875.0,
        grid_import_cum_wh: 5_000.0,
        grid_export_cum_wh: 1_120.0,
    };

    // ── Step 1: power_sample insert (every tick) ──────────────────────────────
    ps_store::insert(&pool, now, 3500.0, 2000.0, -500.0)
        .await
        .expect("power_sample insert must succeed");

    // ── Step 2: accumulator.push (in-memory; not stored) ─────────────────────
    // (no DB interaction; omitted here)

    // ── Step 3: boundary_snapshot + energy_window transaction ─────────────────
    // The real scheduler runs this in an explicit sqlx transaction (begin/commit).
    // For characterization purposes we call the same primitives; transaction
    // wrapping is tested separately in boundary_snapshot_test.rs and scheduler_test.rs.
    let snap_outcome = boundary_snapshot::insert(
        &pool,
        PREV_BOUNDARY,
        curr.production_wh,
        curr.grid_import_cum_wh,
        curr.grid_export_cum_wh,
        now,
        r#"{"meters":[]}"#,
    )
    .await
    .expect("boundary_snapshot insert must succeed");
    assert_eq!(
        snap_outcome,
        boundary_snapshot::InsertOutcome::Inserted,
        "first insert must return Inserted"
    );

    let window = compute_delta(PREV_BOUNDARY, &prev, &curr, true);
    ew_store::insert(&pool, &window)
        .await
        .expect("energy_window insert must succeed");

    // ── Step 4: inverter snapshots ────────────────────────────────────────────
    let inverters = vec![
        MicroinverterSnapshot {
            id: 0,
            window_start: PREV_BOUNDARY,
            serial_number: "SN000001".to_string(),
            watts_output: 320.0,
            is_online: true,
            last_report_date: PREV_BOUNDARY - 60,
        },
        MicroinverterSnapshot {
            id: 0,
            window_start: PREV_BOUNDARY,
            serial_number: "SN000002".to_string(),
            watts_output: 315.0,
            is_online: true,
            last_report_date: PREV_BOUNDARY - 60,
        },
    ];
    inv_store::insert_batch(&pool, &inverters)
        .await
        .expect("inverter_snapshot insert_batch must succeed");

    // ── Step 5: phase readings ────────────────────────────────────────────────
    let phase_rows = vec![
        PhaseReading {
            id: 0,
            sampled_at: now,
            meter_eid: 704643328,
            channel_eid: 1778385169,
            active_power_w_at_boundary: 3500.0,
            energy_dlvd_wh: 100_875.0,
            energy_rcvd_wh: 0.0,
        },
        PhaseReading {
            id: 0,
            sampled_at: now,
            meter_eid: 704643584,
            channel_eid: 1778385170,
            active_power_w_at_boundary: -500.0,
            energy_dlvd_wh: 5_000.0,
            energy_rcvd_wh: 1_120.0,
        },
    ];
    phase_store::insert_batch(&pool, &phase_rows)
        .await
        .expect("phase_reading insert_batch must succeed");

    // ── Step 6: persist cumulative reading (config_store) ────────────────────
    config_store::set(&pool, "last_poll_timestamp", &now.to_string())
        .await
        .expect("config_store set last_poll_timestamp");
    config_store::set(
        &pool,
        "last_cumulative_production_wh",
        &curr.production_wh.to_string(),
    )
    .await
    .expect("config_store set last_cumulative_production_wh");
    config_store::set(
        &pool,
        "last_cumulative_grid_import_wh",
        &curr.grid_import_cum_wh.to_string(),
    )
    .await
    .expect("config_store set last_cumulative_grid_import_wh");
    config_store::set(
        &pool,
        "last_cumulative_grid_export_wh",
        &curr.grid_export_cum_wh.to_string(),
    )
    .await
    .expect("config_store set last_cumulative_grid_export_wh");
    config_store::set(&pool, "last_window_start", &PREV_BOUNDARY.to_string())
        .await
        .expect("config_store set last_window_start");

    // ── Assertions: row counts ────────────────────────────────────────────────
    let ew_rows = ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::All)
        .await
        .expect("energy_window query");
    assert_eq!(
        ew_rows.len(),
        1,
        "exactly 1 energy_window row after one boundary crossing"
    );

    let ps_rows = ps_store::query_range(&pool, 0, i64::MAX, 100, 0)
        .await
        .expect("power_sample query");
    assert_eq!(
        ps_rows.len(),
        1,
        "exactly 1 power_sample row (the boundary tick)"
    );

    let inv_rows = inv_store::query_by_window(&pool, PREV_BOUNDARY)
        .await
        .expect("microinverter_snapshot query");
    assert_eq!(
        inv_rows.len(),
        2,
        "exactly 2 microinverter_snapshot rows for the boundary window"
    );

    let phase_result = phase_store::query_range(&pool, now, now + 1, None, 100, 0)
        .await
        .expect("phase_reading query");
    assert_eq!(
        phase_result.len(),
        2,
        "exactly 2 phase_reading rows at boundary tick"
    );

    // ── Assertions: config_store keys ─────────────────────────────────────────
    let ts_stored = config_store::get(&pool, "last_poll_timestamp")
        .await
        .expect("get last_poll_timestamp")
        .expect("last_poll_timestamp must exist");
    assert_eq!(
        ts_stored,
        now.to_string(),
        "last_poll_timestamp must match tick timestamp"
    );

    let prod_stored = config_store::get(&pool, "last_cumulative_production_wh")
        .await
        .expect("get last_cumulative_production_wh")
        .expect("last_cumulative_production_wh must exist");
    assert_eq!(
        prod_stored,
        curr.production_wh.to_string(),
        "last_cumulative_production_wh must match curr"
    );

    let import_stored = config_store::get(&pool, "last_cumulative_grid_import_wh")
        .await
        .expect("get last_cumulative_grid_import_wh")
        .expect("last_cumulative_grid_import_wh must exist");
    assert_eq!(
        import_stored,
        curr.grid_import_cum_wh.to_string(),
        "last_cumulative_grid_import_wh must match curr"
    );

    let export_stored = config_store::get(&pool, "last_cumulative_grid_export_wh")
        .await
        .expect("get last_cumulative_grid_export_wh")
        .expect("last_cumulative_grid_export_wh must exist");
    assert_eq!(
        export_stored,
        curr.grid_export_cum_wh.to_string(),
        "last_cumulative_grid_export_wh must match curr"
    );

    let ws_stored = config_store::get(&pool, "last_window_start")
        .await
        .expect("get last_window_start")
        .expect("last_window_start must exist");
    assert_eq!(
        ws_stored,
        PREV_BOUNDARY.to_string(),
        "last_window_start must match the closed boundary"
    );
}
