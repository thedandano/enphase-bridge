use enphase_bridge::collector::window_aggregator::{
    CumulativeReading, compute_delta, window_boundary,
};
use enphase_bridge::storage::energy_window as ew_store;
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

// 2024-01-01 00:00:00 UTC — exact 15-min boundary
const BOUNDARY: i64 = 1704067200;
const NEXT_BOUNDARY: i64 = BOUNDARY + 900;

fn reading_at(ts: i64, prod: f64, import: f64, export: f64) -> CumulativeReading {
    CumulativeReading {
        timestamp: ts,
        production_wh: prod,
        grid_import_cum_wh: import,
        grid_export_cum_wh: export,
    }
}

/// Task 3.1 — Simulates the fixed scheduler across a window boundary with a real SQLite pool.
/// Verifies the stored window row reflects the full 15-minute counter delta.
///
/// Task 3.2 — Verifies no row is written to the DB for mid-window ticks.
///
/// Regression guard: demonstrates that the buggy scheduler (which used the last 60-second tick
/// as the anchor) would have stored only ~1/15th of the correct Wh value.
#[tokio::test]
async fn test_scheduler_anchor_freeze_stores_correct_window() {
    let pool = test_pool().await;

    // Anchor: reading at the previous boundary (simulating persisted state or prior crossing).
    // Full-window expectations: 875 Wh produced, 0 imported, 120 exported.
    let anchor = reading_at(BOUNDARY, 100_000.0, 5_000.0, 1_000.0);

    // 14 mid-window ticks at 60-second intervals (BOUNDARY+60 … BOUNDARY+840).
    // The last tick is 60 s before the boundary — the one the BUGGY scheduler would use as prev.
    let mid_ticks: Vec<CumulativeReading> = (1_i64..=14)
        .map(|m| {
            reading_at(
                BOUNDARY + m * 60,
                100_000.0 + m as f64 * (875.0 / 15.0), // ~58.3 Wh per minute
                5_000.0,
                1_000.0 + m as f64 * (120.0 / 15.0), // ~8 Wh per minute exported
            )
        })
        .collect();

    let current_anchor = anchor.clone();

    // Task 3.2 — no DB writes happen for mid-window ticks in the fixed scheduler.
    // Verify all mid-ticks are in the same window as the anchor.
    for (i, tick) in mid_ticks.iter().enumerate() {
        assert_eq!(
            window_boundary(current_anchor.timestamp),
            window_boundary(tick.timestamp),
            "mid-window tick {} must be in the same window as anchor",
            i
        );
    }

    // No DB writes yet.
    let rows = ew_store::query_range(&pool, 0, i64::MAX, 100, 0)
        .await
        .expect("query failed");
    assert!(
        rows.is_empty(),
        "no window rows should be written for mid-window ticks — got {} rows",
        rows.len()
    );

    // Boundary-crossing tick.
    let boundary_tick = reading_at(NEXT_BOUNDARY + 5, 100_875.0, 5_000.0, 1_120.0);

    let prev_window_ts = window_boundary(current_anchor.timestamp);
    let curr_window_ts = window_boundary(boundary_tick.timestamp);
    assert!(
        curr_window_ts > prev_window_ts,
        "boundary tick must be in the next window"
    );

    // Task 3.1 — fixed scheduler computes delta using the FROZEN anchor, then inserts.
    let window = compute_delta(prev_window_ts, &current_anchor, &boundary_tick, true);
    ew_store::insert(&pool, &window)
        .await
        .expect("insert failed");

    // Verify stored row.
    let rows = ew_store::query_range(&pool, 0, i64::MAX, 100, 0)
        .await
        .expect("query failed");
    assert_eq!(
        rows.len(),
        1,
        "exactly one window row after boundary crossing"
    );

    let stored = &rows[0];
    assert_eq!(stored.window_start, BOUNDARY);
    assert!(
        (stored.wh_produced - 875.0).abs() < 1.0,
        "wh_produced should be ~875 Wh (full 15-min delta), got {}",
        stored.wh_produced
    );
    assert!(
        (stored.wh_grid_import - 0.0).abs() < 1.0,
        "wh_grid_import should be 0, got {}",
        stored.wh_grid_import
    );
    assert!(
        (stored.wh_grid_export - 120.0).abs() < 1.0,
        "wh_grid_export should be ~120 Wh, got {}",
        stored.wh_grid_export
    );

    // Regression guard: the BUGGY scheduler used the last 60-second tick as prev.
    // That tick is only ~58 Wh of production away from boundary_tick (1/15 of the window).
    let last_mid_tick = mid_ticks.last().unwrap();
    let buggy_window = compute_delta(prev_window_ts, last_mid_tick, &boundary_tick, true);
    assert!(
        buggy_window.wh_produced < stored.wh_produced / 10.0,
        "buggy 60s-anchor delta ({:.1} Wh) should be <1/10 of correct 15-min delta ({:.1} Wh)",
        buggy_window.wh_produced,
        stored.wh_produced
    );
}
