use enphase_bridge::collector::window_aggregator::CURRENT_FORMULA_VERSION;
use enphase_bridge::storage::energy_window::{self as ew_store, FormulaFilter};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

// 2024-01-01 00:00:00 UTC — exact 15-min boundary
const BOUNDARY: i64 = 1704067200;

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

/// Insert a minimal energy_window row with an explicit formula_version.
/// Uses `window_start` as the unique key — callers must pass distinct values.
async fn insert_with_version(pool: &SqlitePool, window_start: i64, formula_version: i32) {
    sqlx::query(
        "INSERT INTO energy_window \
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(window_start)
    .bind(0.0_f64)
    .bind(0.0_f64)
    .bind(0.0_f64)
    .bind(0.0_f64)
    .bind(true)
    .bind(formula_version)
    .execute(pool)
    .await
    .expect("insert");
}

/// Task 11.1 — CURRENT_FORMULA_VERSION is positive non-zero.
///
/// With CURRENT=1, `query_stale` (formula_version > 0 AND < 1) is an empty integer range.
/// This test documents that real system behavior:
/// - asserts CURRENT >= 1
/// - inserts a formula_version=0 row, verifies query_stale returns empty
/// - verifies FormulaFilter::CurrentOnly excludes the formula_version=0 row
#[tokio::test]
async fn test_current_formula_version_positive_and_stale_empty_with_current_one() {
    // CURRENT_FORMULA_VERSION must not be the unrecomputable sentinel (0).
    assert_ne!(
        CURRENT_FORMULA_VERSION, 0,
        "CURRENT_FORMULA_VERSION must be non-zero"
    );

    let pool = test_pool().await;

    // Insert a formula_version=0 row (unrecomputable, not stale).
    insert_with_version(&pool, BOUNDARY, 0).await;

    // query_stale: formula_version > 0 AND formula_version < CURRENT_FORMULA_VERSION.
    // With CURRENT=1 this is an empty set — no valid integer satisfies formula_version > 0 AND < 1.
    let stale = ew_store::query_stale(&pool)
        .await
        .expect("query_stale failed");
    assert!(
        stale.is_empty(),
        "query_stale must return empty with CURRENT_FORMULA_VERSION={} (no integer satisfies > 0 AND < {})",
        CURRENT_FORMULA_VERSION,
        CURRENT_FORMULA_VERSION
    );

    // FormulaFilter::CurrentOnly excludes the formula_version=0 row.
    let current_only =
        ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::CurrentOnly)
            .await
            .expect("query_range(CurrentOnly) failed");
    assert!(
        current_only.is_empty(),
        "FormulaFilter::CurrentOnly must exclude formula_version=0 rows, got {} rows",
        current_only.len()
    );
}

/// Task 11.2 — FormulaFilter behavior across all three variants.
///
/// Seed two rows:
///   - window 0: formula_version = 0  (unrecomputable)
///   - window 1: formula_version = CURRENT_FORMULA_VERSION  (current / recomputable)
///
/// Expected per filter:
///   FormulaFilter::All          → both rows
///   FormulaFilter::CurrentOnly  → window 1 only
///   FormulaFilter::Recomputable → window 1 only  (formula_version > 0)
#[tokio::test]
async fn test_formula_filter_variants() {
    let pool = test_pool().await;

    // Row with formula_version = 0 (unrecomputable).
    insert_with_version(&pool, BOUNDARY, 0).await;
    // Row with formula_version = CURRENT_FORMULA_VERSION (current).
    insert_with_version(&pool, BOUNDARY + 900, CURRENT_FORMULA_VERSION).await;

    // All — both rows returned.
    let all = ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::All)
        .await
        .expect("query_range(All) failed");
    assert_eq!(
        all.len(),
        2,
        "FormulaFilter::All must return both rows, got {}",
        all.len()
    );

    // CurrentOnly — only the row with formula_version = CURRENT_FORMULA_VERSION.
    let current_only =
        ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::CurrentOnly)
            .await
            .expect("query_range(CurrentOnly) failed");
    assert_eq!(
        current_only.len(),
        1,
        "FormulaFilter::CurrentOnly must return exactly 1 row, got {}",
        current_only.len()
    );
    assert_eq!(
        current_only[0].formula_version, CURRENT_FORMULA_VERSION,
        "FormulaFilter::CurrentOnly must return the CURRENT row"
    );
    assert_eq!(
        current_only[0].window_start,
        BOUNDARY + 900,
        "FormulaFilter::CurrentOnly must exclude the formula_version=0 row"
    );

    // Recomputable — formula_version > 0, so only CURRENT row (version=0 is excluded).
    let recomputable =
        ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::Recomputable)
            .await
            .expect("query_range(Recomputable) failed");
    assert_eq!(
        recomputable.len(),
        1,
        "FormulaFilter::Recomputable must return exactly 1 row, got {}",
        recomputable.len()
    );
    assert_eq!(
        recomputable[0].formula_version, CURRENT_FORMULA_VERSION,
        "FormulaFilter::Recomputable must return the formula_version > 0 row"
    );
    assert_eq!(
        recomputable[0].window_start,
        BOUNDARY + 900,
        "FormulaFilter::Recomputable must exclude the formula_version=0 row"
    );
}

/// Task 11.3 — count_unrecomputable and count_stale.
///
/// Seed three rows:
///   - window 0: formula_version = 0  (unrecomputable)
///   - window 1: formula_version = 0  (unrecomputable)
///   - window 2: formula_version = CURRENT_FORMULA_VERSION  (current)
///
/// Expected counts:
///   count_unrecomputable → 2  (formula_version = 0)
///   count_stale          → 0  (formula_version > 0 AND < CURRENT; empty range with CURRENT=1)
#[tokio::test]
async fn test_count_unrecomputable_and_count_stale() {
    let pool = test_pool().await;

    // Two unrecomputable rows (formula_version = 0).
    insert_with_version(&pool, BOUNDARY, 0).await;
    insert_with_version(&pool, BOUNDARY + 900, 0).await;
    // One current row (formula_version = CURRENT_FORMULA_VERSION).
    insert_with_version(&pool, BOUNDARY + 1800, CURRENT_FORMULA_VERSION).await;

    let unrecomputable = ew_store::count_unrecomputable(&pool)
        .await
        .expect("count_unrecomputable failed");
    assert_eq!(
        unrecomputable, 2,
        "count_unrecomputable must count exactly the formula_version=0 rows, got {}",
        unrecomputable
    );

    // With CURRENT_FORMULA_VERSION=1, stale = formula_version > 0 AND < 1 — empty integer range.
    let stale = ew_store::count_stale(&pool)
        .await
        .expect("count_stale failed");
    assert_eq!(
        stale, 0,
        "count_stale must be 0 with CURRENT_FORMULA_VERSION={} (no integer satisfies > 0 AND < {}), got {}",
        CURRENT_FORMULA_VERSION, CURRENT_FORMULA_VERSION, stale
    );
}
