use enphase_bridge::storage::models::PhaseReading;
use enphase_bridge::storage::phase_reading;
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

fn make_reading(sampled_at: i64, meter_eid: i64, channel_eid: i64) -> PhaseReading {
    PhaseReading {
        id: 0,
        sampled_at,
        meter_eid,
        channel_eid,
        active_power_w_at_boundary: 500.0,
        energy_dlvd_wh: 12345.0,
        energy_rcvd_wh: 0.0,
    }
}

/// 7.3a — insert_batch stores rows that are retrievable via query_range.
#[tokio::test]
async fn test_insert_batch_and_query() {
    let pool = test_pool().await;
    let rows = vec![
        make_reading(1000, 704643328, 1778385169),
        make_reading(1000, 704643328, 1778385170),
    ];
    phase_reading::insert_batch(&pool, &rows).await.unwrap();

    let result = phase_reading::query_range(&pool, 900, 1100, None, 10, 0)
        .await
        .unwrap();
    assert_eq!(result.len(), 2, "should return both inserted rows");
    assert_eq!(result[0].meter_eid, 704643328);
    assert_eq!(result[0].channel_eid, 1778385169);
    assert!(
        (result[0].energy_dlvd_wh - 12345.0).abs() < 1e-6,
        "energy_dlvd_wh mismatch"
    );
}

/// 7.3b — second insert of same (sampled_at, meter_eid, channel_eid) is silently ignored (INSERT OR IGNORE).
#[tokio::test]
async fn test_duplicate_insert_ignored() {
    let pool = test_pool().await;
    let row = make_reading(2000, 704643328, 1778385169);
    phase_reading::insert_batch(&pool, std::slice::from_ref(&row))
        .await
        .unwrap();
    // Second insert of the same unique key must not error or create a duplicate.
    phase_reading::insert_batch(&pool, &[row]).await.unwrap();

    let result = phase_reading::query_range(&pool, 1900, 2100, None, 10, 0)
        .await
        .unwrap();
    assert_eq!(
        result.len(),
        1,
        "duplicate insert must be ignored — exactly one row expected"
    );
}

/// 7.3c — query_range with meter_eid filter returns only rows for that meter.
#[tokio::test]
async fn test_query_filter_by_meter_eid() {
    let pool = test_pool().await;
    let rows = vec![
        make_reading(3000, 704643328, 1778385169),
        make_reading(3000, 704643584, 1778385171),
    ];
    phase_reading::insert_batch(&pool, &rows).await.unwrap();

    let filtered = phase_reading::query_range(&pool, 2900, 3100, Some(704643328), 10, 0)
        .await
        .unwrap();
    assert_eq!(
        filtered.len(),
        1,
        "filter by meter_eid should return only matching row"
    );
    assert_eq!(filtered[0].meter_eid, 704643328);
    assert_eq!(filtered[0].channel_eid, 1778385169);
}

/// 7.6 — delete_before removes rows with sampled_at < cutoff and leaves newer rows intact.
#[tokio::test]
async fn test_delete_before() {
    let pool = test_pool().await;
    let rows = vec![
        make_reading(100, 704643328, 1778385169), // old — should be deleted
        make_reading(200, 704643328, 1778385170), // old — should be deleted
        make_reading(5000, 704643328, 1778385169), // new — must survive
    ];
    phase_reading::insert_batch(&pool, &rows).await.unwrap();

    let deleted = phase_reading::delete_before(&pool, 300).await.unwrap();
    assert_eq!(deleted, 2, "should have deleted the 2 old rows");

    let remaining = phase_reading::query_range(&pool, 0, 10000, None, 10, 0)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1, "one row should remain after deletion");
    assert_eq!(remaining[0].sampled_at, 5000, "the newer row must survive");
}
