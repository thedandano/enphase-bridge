use enphase_bridge::collector::window_aggregator::{CumulativeReading, compute_delta};
use enphase_bridge::storage::energy_window::FormulaFilter;
use enphase_bridge::storage::{energy_window as ew_store, power_sample as ps_store};
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

/// 6.2 — after two power_sample inserts, query_range returns both rows with correct values.
#[tokio::test]
async fn test_power_sample_insert_and_query_two_rows() {
    let pool = test_pool().await;
    let t1: i64 = 1704067260;
    let t2: i64 = 1704067320;

    ps_store::insert(&pool, t1, 2000.0, 1500.0, -500.0)
        .await
        .unwrap();
    ps_store::insert(&pool, t2, 2100.0, 1600.0, -600.0)
        .await
        .unwrap();

    let rows = ps_store::query_range(&pool, t1, t2 + 1, 100, 0)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "should have 2 rows after two inserts");

    let r1 = rows
        .iter()
        .find(|r| r.sampled_at == t1)
        .expect("t1 row missing");
    assert!(
        (r1.production_w - 2000.0).abs() < 1e-6,
        "production_w mismatch at t1"
    );
    assert!(
        (r1.consumption_w - 1500.0).abs() < 1e-6,
        "consumption_w mismatch at t1"
    );
    assert!((r1.grid_w - (-500.0)).abs() < 1e-6, "grid_w mismatch at t1");

    let r2 = rows
        .iter()
        .find(|r| r.sampled_at == t2)
        .expect("t2 row missing");
    assert!(
        (r2.production_w - 2100.0).abs() < 1e-6,
        "production_w mismatch at t2"
    );
}

/// 6.3 — power_sample insert failure does not prevent energy_window write.
/// Tests error isolation: ps_store failure is contained and ew_store still succeeds.
#[tokio::test]
async fn test_power_sample_insert_failure_does_not_prevent_window_write() {
    let pool = test_pool().await;

    // Drop the power_sample table to force insert failures
    sqlx::query("DROP TABLE power_sample")
        .execute(&pool)
        .await
        .unwrap();

    // ps_store::insert should now fail
    let ps_result = ps_store::insert(&pool, 1704067260, 2000.0, 1500.0, -500.0).await;
    assert!(ps_result.is_err(), "insert must fail when table is absent");

    // ew_store::insert must still succeed independently
    let prev = CumulativeReading {
        timestamp: 1704067200,
        production_wh: 1000.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 0.0,
    };
    let curr = CumulativeReading {
        timestamp: 1704068100,
        production_wh: 1100.0,
        grid_import_cum_wh: 0.0,
        grid_export_cum_wh: 0.0,
    };
    let window = compute_delta(1704067200, &prev, &curr, true);
    let ew_result = ew_store::insert(&pool, &window).await;
    assert!(
        ew_result.is_ok(),
        "energy_window insert must succeed even when power_sample fails"
    );

    let rows = ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::All)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "energy_window row must be present");
}

/// 6.7 — delete_before removes rows older than cutoff and leaves newer rows intact.
#[tokio::test]
async fn test_power_sample_delete_before_cutoff() {
    let pool = test_pool().await;
    let old_t: i64 = 1000;
    let cutoff: i64 = 2000;
    let new_t: i64 = 3000;

    ps_store::insert(&pool, old_t, 100.0, 80.0, 20.0)
        .await
        .unwrap();
    ps_store::insert(&pool, new_t, 200.0, 150.0, 50.0)
        .await
        .unwrap();

    let deleted = ps_store::delete_before(&pool, cutoff).await.unwrap();
    assert_eq!(
        deleted, 1,
        "should have deleted exactly 1 row (the old one)"
    );

    let rows = ps_store::query_range(&pool, 0, i64::MAX, 100, 0)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "one row should remain after deletion");
    assert_eq!(rows[0].sampled_at, new_t, "the newer row must survive");
}
