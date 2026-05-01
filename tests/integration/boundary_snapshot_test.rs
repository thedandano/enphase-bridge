use enphase_bridge::storage::boundary_snapshot::{self, InsertOutcome};
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

/// 11.4 — Migration: rows seeded before the cutoff (window_start < 1746057600) must have
/// formula_version = 0; no phantom rows are created by the migration for future timestamps.
#[tokio::test]
async fn test_migration_formula_version_backfill() {
    let pool = test_pool().await;

    // The migration runs automatically. Seed a pre-cutoff energy_window row via raw SQL,
    // omitting formula_version to exercise the column DEFAULT 0 defined by migration 002.
    let pre_cutoff_ts: i64 = 1746057599; // one second before the cutoff
    sqlx::query(
        "INSERT INTO energy_window (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete)
         VALUES (?, 100.0, 80.0, 10.0, 30.0, 1)",
    )
    .bind(pre_cutoff_ts)
    .execute(&pool)
    .await
    .expect("insert pre-cutoff row");

    // Assert formula_version = 0 for the pre-cutoff row (column DEFAULT from migration 002).
    let row: (i32,) =
        sqlx::query_as("SELECT formula_version FROM energy_window WHERE window_start = ?")
            .bind(pre_cutoff_ts)
            .fetch_one(&pool)
            .await
            .expect("fetch pre-cutoff row");
    assert_eq!(row.0, 0, "pre-cutoff row must have formula_version = 0");

    // Assert no phantom rows exist at or after the cutoff.
    let future_ts: i64 = 1746057600; // exactly the cutoff
    let phantom: Option<(i64,)> =
        sqlx::query_as("SELECT window_start FROM energy_window WHERE window_start >= ?")
            .bind(future_ts)
            .fetch_optional(&pool)
            .await
            .expect("query future rows");
    assert!(
        phantom.is_none(),
        "migration must not create phantom rows for future timestamps"
    );
}

/// 11.11 — `boundary_snapshot::insert` returns `InsertOutcome::Inserted` on first write and
/// `InsertOutcome::AlreadyExists` on a duplicate `window_start`.
#[tokio::test]
async fn test_insert_outcome_idempotent() {
    let pool = test_pool().await;

    let window_start: i64 = 1704067200; // 2024-01-01 00:00:00 UTC

    let first = boundary_snapshot::insert(
        &pool,
        window_start,
        500.0,
        1000.0,
        200.0,
        window_start + 1,
        r#"{"meters":[]}"#,
    )
    .await
    .expect("first insert");
    assert_eq!(
        first,
        InsertOutcome::Inserted,
        "first insert must return Inserted"
    );

    let second = boundary_snapshot::insert(
        &pool,
        window_start,
        999.0, // different values — should be ignored
        9999.0,
        999.0,
        window_start + 2,
        r#"{"meters":["dup"]}"#,
    )
    .await
    .expect("second insert");
    assert_eq!(
        second,
        InsertOutcome::AlreadyExists,
        "duplicate insert must return AlreadyExists"
    );
}

/// 11.13 — `boundary_snapshot::query_pair` returns `None` when the preceding snapshot is not
/// exactly 900 seconds before `window_start` (gap case), and returns `Some((prev, curr))` when
/// both T-900 and T exist (success case).
#[tokio::test]
async fn test_query_pair_adjacency_requirement() {
    let pool = test_pool().await;

    let t: i64 = 1704067200; // 2024-01-01 00:00:00 UTC

    // --- Gap case: insert at T and T-1800 (not adjacent — gap of 1800 s instead of 900 s) ---
    boundary_snapshot::insert(&pool, t, 100.0, 1000.0, 50.0, t + 1, r#"{}"#)
        .await
        .expect("insert at T");
    boundary_snapshot::insert(&pool, t - 1800, 80.0, 990.0, 45.0, t - 1799, r#"{}"#)
        .await
        .expect("insert at T-1800");

    let result = boundary_snapshot::query_pair(&pool, t)
        .await
        .expect("query_pair gap case");
    assert!(
        result.is_none(),
        "query_pair must return None when prev is at T-1800, not T-900"
    );

    // --- Success case: insert at T-900 so the pair is now complete ---
    boundary_snapshot::insert(&pool, t - 900, 90.0, 995.0, 48.0, t - 899, r#"{}"#)
        .await
        .expect("insert at T-900");

    let result = boundary_snapshot::query_pair(&pool, t)
        .await
        .expect("query_pair success case");
    let (prev, curr) = result.expect("query_pair must return Some when both T-900 and T exist");

    assert_eq!(
        prev.window_start,
        t - 900,
        "prev.window_start must be T-900"
    );
    assert_eq!(curr.window_start, t, "curr.window_start must be T");
}
