use enphase_bridge::collector::gateway_client::extract_cumulatives_from_json;
use enphase_bridge::collector::window_aggregator::{
    CURRENT_FORMULA_VERSION, CumulativeReading, compute_delta,
};
use enphase_bridge::storage::energy_window::FormulaFilter;
use enphase_bridge::storage::{boundary_snapshot, energy_window};
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
        .expect("pool");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    pool
}

// Cumulative values shared across 11.14 and 11.15.
// prev (at window_start - 900): production=100_000.0, import=5_000.0, export=1_000.0
// curr (at window_start):       production=100_875.0, import=5_000.0, export=1_120.0
//
// Expected delta:
//   wh_produced    = 100_875.0 - 100_000.0 = 875.0
//   wh_grid_import =   5_000.0 -   5_000.0 =   0.0
//   wh_grid_export =   1_120.0 -   1_000.0 = 120.0
//   wh_consumed    = 875.0 + 0.0 - 120.0   = 755.0
const PREV_PRODUCTION: f64 = 100_000.0;
const PREV_IMPORT: f64 = 5_000.0;
const PREV_EXPORT: f64 = 1_000.0;
const CURR_PRODUCTION: f64 = 100_875.0;
const CURR_IMPORT: f64 = 5_000.0;
const CURR_EXPORT: f64 = 1_120.0;

const EXPECTED_WH_PRODUCED: f64 = 875.0;
const EXPECTED_WH_IMPORT: f64 = 0.0;
const EXPECTED_WH_EXPORT: f64 = 120.0;
const EXPECTED_WH_CONSUMED: f64 = 755.0;

// EID_PRODUCTION = 704643328 (actEnergyDlvd → production_cum_wh)
// EID_CONSUMPTION = 704643584 (actEnergyDlvd → grid_import_cum_wh, actEnergyRcvd → grid_export_cum_wh)
fn make_raw_json(
    production_cum_wh: f64,
    grid_import_cum_wh: f64,
    grid_export_cum_wh: f64,
) -> String {
    format!(
        r#"[
  {{
    "eid": 704643328,
    "activePower": 0.0,
    "actEnergyDlvd": {production_cum_wh},
    "actEnergyRcvd": 0.0
  }},
  {{
    "eid": 704643584,
    "activePower": 0.0,
    "actEnergyDlvd": {grid_import_cum_wh},
    "actEnergyRcvd": {grid_export_cum_wh}
  }}
]"#
    )
}

/// 11.14 — `run_typed` logic: re-derives wh_* from typed boundary_snapshot columns.
///
/// Verifies:
/// a) `boundary_snapshot::query_pair` returns the adjacent pair correctly.
/// b) `compute_delta` produces the expected delta values.
/// c) `energy_window::update_recomputed` persists those delta values and bumps formula_version.
#[tokio::test]
async fn test_run_typed_update_recomputed() {
    let pool = test_pool().await;

    let window_start: i64 = 1746057600; // at/after migration cutoff → formula_version default is 0, but we bind 1
    let prev_start = window_start - 900;

    // Insert the energy_window row with formula_version=1 (current, so query_stale won't find it,
    // but we test the update path directly).
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version)
         VALUES (?, 100.0, 80.0, 10.0, 5.0, 1, 1)",
    )
    .bind(window_start)
    .execute(&pool)
    .await
    .expect("insert energy_window");

    // Insert prev boundary_snapshot at window_start - 900.
    boundary_snapshot::insert(
        &pool,
        prev_start,
        PREV_PRODUCTION,
        PREV_IMPORT,
        PREV_EXPORT,
        prev_start + 1,
        "{}",
    )
    .await
    .expect("insert prev boundary_snapshot");

    // Insert curr boundary_snapshot at window_start.
    boundary_snapshot::insert(
        &pool,
        window_start,
        CURR_PRODUCTION,
        CURR_IMPORT,
        CURR_EXPORT,
        window_start + 1,
        "{}",
    )
    .await
    .expect("insert curr boundary_snapshot");

    // Verify query_pair returns the adjacent pair.
    let pair = boundary_snapshot::query_pair(&pool, window_start)
        .await
        .expect("query_pair")
        .expect("pair must exist");

    let (prev, curr) = pair;
    assert_eq!(
        prev.window_start, prev_start,
        "prev.window_start must be window_start - 900"
    );
    assert_eq!(
        curr.window_start, window_start,
        "curr.window_start must be window_start"
    );

    // Build CumulativeReadings from typed snapshot columns (as run_typed does).
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

    // Compute delta.
    let delta = compute_delta(window_start, &prev_r, &curr_r, true);
    assert!(
        (delta.wh_produced - EXPECTED_WH_PRODUCED).abs() < 1e-9,
        "wh_produced: expected {EXPECTED_WH_PRODUCED}, got {}",
        delta.wh_produced
    );
    assert!(
        (delta.wh_grid_import - EXPECTED_WH_IMPORT).abs() < 1e-9,
        "wh_grid_import: expected {EXPECTED_WH_IMPORT}, got {}",
        delta.wh_grid_import
    );
    assert!(
        (delta.wh_grid_export - EXPECTED_WH_EXPORT).abs() < 1e-9,
        "wh_grid_export: expected {EXPECTED_WH_EXPORT}, got {}",
        delta.wh_grid_export
    );
    assert!(
        (delta.wh_consumed - EXPECTED_WH_CONSUMED).abs() < 1e-9,
        "wh_consumed: expected {EXPECTED_WH_CONSUMED}, got {}",
        delta.wh_consumed
    );

    // Apply update_recomputed and verify all five fields are persisted correctly.
    energy_window::update_recomputed(
        &pool,
        window_start,
        delta.wh_produced,
        delta.wh_consumed,
        delta.wh_grid_import,
        delta.wh_grid_export,
        CURRENT_FORMULA_VERSION,
        delta.was_clamped,
    )
    .await
    .expect("update_recomputed");

    let row: (f64, f64, f64, f64, i32) = sqlx::query_as(
        "SELECT wh_produced, wh_consumed, wh_grid_import, wh_grid_export, formula_version
         FROM energy_window WHERE window_start = ?",
    )
    .bind(window_start)
    .fetch_one(&pool)
    .await
    .expect("fetch updated row");

    let (wh_produced, wh_consumed, wh_grid_import, wh_grid_export, formula_version) = row;
    assert!(
        (wh_produced - EXPECTED_WH_PRODUCED).abs() < 1e-9,
        "persisted wh_produced"
    );
    assert!(
        (wh_consumed - EXPECTED_WH_CONSUMED).abs() < 1e-9,
        "persisted wh_consumed"
    );
    assert!(
        (wh_grid_import - EXPECTED_WH_IMPORT).abs() < 1e-9,
        "persisted wh_grid_import"
    );
    assert!(
        (wh_grid_export - EXPECTED_WH_EXPORT).abs() < 1e-9,
        "persisted wh_grid_export"
    );
    assert_eq!(
        formula_version, CURRENT_FORMULA_VERSION,
        "formula_version must be bumped to CURRENT"
    );
}

/// 11.14 (dry-run simulation) — Computing the delta but NOT calling `update_recomputed` leaves
/// the row unchanged (simulates --dry-run printing without writing).
#[tokio::test]
async fn test_run_typed_dry_run_leaves_row_unchanged() {
    let pool = test_pool().await;

    let window_start: i64 = 1746057600;
    let prev_start = window_start - 900;

    // Insert energy_window row with known original values.
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version)
         VALUES (?, 100.0, 80.0, 10.0, 5.0, 1, 1)",
    )
    .bind(window_start)
    .execute(&pool)
    .await
    .expect("insert energy_window");

    // Insert adjacent boundary snapshots.
    boundary_snapshot::insert(
        &pool,
        prev_start,
        PREV_PRODUCTION,
        PREV_IMPORT,
        PREV_EXPORT,
        prev_start + 1,
        "{}",
    )
    .await
    .expect("insert prev boundary_snapshot");
    boundary_snapshot::insert(
        &pool,
        window_start,
        CURR_PRODUCTION,
        CURR_IMPORT,
        CURR_EXPORT,
        window_start + 1,
        "{}",
    )
    .await
    .expect("insert curr boundary_snapshot");

    // Compute delta (dry-run: log only, do not call update_recomputed).
    let pair = boundary_snapshot::query_pair(&pool, window_start)
        .await
        .expect("query_pair")
        .expect("pair must exist");
    let (prev, curr) = pair;
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
    let _delta = compute_delta(window_start, &prev_r, &curr_r, true);
    // NOT calling update_recomputed — simulates --dry-run.

    // Verify the row is unchanged.
    let row: (f64, f64, f64, f64, i32) = sqlx::query_as(
        "SELECT wh_produced, wh_consumed, wh_grid_import, wh_grid_export, formula_version
         FROM energy_window WHERE window_start = ?",
    )
    .bind(window_start)
    .fetch_one(&pool)
    .await
    .expect("fetch row");

    let (wh_produced, wh_consumed, wh_grid_import, wh_grid_export, formula_version) = row;
    assert!(
        (wh_produced - 100.0).abs() < 1e-9,
        "wh_produced must be unchanged (100.0), got {wh_produced}"
    );
    assert!(
        (wh_consumed - 80.0).abs() < 1e-9,
        "wh_consumed must be unchanged (80.0), got {wh_consumed}"
    );
    assert!(
        (wh_grid_import - 10.0).abs() < 1e-9,
        "wh_grid_import must be unchanged (10.0), got {wh_grid_import}"
    );
    assert!(
        (wh_grid_export - 5.0).abs() < 1e-9,
        "wh_grid_export must be unchanged (5.0), got {wh_grid_export}"
    );
    assert_eq!(
        formula_version, 1,
        "formula_version must be unchanged (1), got {formula_version}"
    );
}

/// 11.15 — `run_raw` logic: re-parses raw_meters_json via `extract_cumulatives_from_json` and
/// produces wh_* matching what run_typed would produce (same cumulative values → same delta).
///
/// Also verifies that `query_range` with `FormulaFilter::Recomputable` excludes formula_version=0 rows.
#[tokio::test]
async fn test_run_raw_extract_and_compute() {
    // Build raw JSON strings carrying the same cumulative values as the typed test.
    let prev_json = make_raw_json(PREV_PRODUCTION, PREV_IMPORT, PREV_EXPORT);
    let curr_json = make_raw_json(CURR_PRODUCTION, CURR_IMPORT, CURR_EXPORT);

    // Parse both snapshots — must succeed.
    let prev_readings = extract_cumulatives_from_json(&prev_json)
        .expect("extract_cumulatives_from_json must succeed on prev_json");
    let curr_readings = extract_cumulatives_from_json(&curr_json)
        .expect("extract_cumulatives_from_json must succeed on curr_json");

    // Verify the parsed cumulatives match the input values.
    assert!(
        (prev_readings.production_cum_wh - PREV_PRODUCTION).abs() < 1e-9,
        "prev production_cum_wh"
    );
    assert!(
        (prev_readings.grid_import_cum_wh - PREV_IMPORT).abs() < 1e-9,
        "prev grid_import_cum_wh"
    );
    assert!(
        (prev_readings.grid_export_cum_wh - PREV_EXPORT).abs() < 1e-9,
        "prev grid_export_cum_wh"
    );
    assert!(
        (curr_readings.production_cum_wh - CURR_PRODUCTION).abs() < 1e-9,
        "curr production_cum_wh"
    );
    assert!(
        (curr_readings.grid_import_cum_wh - CURR_IMPORT).abs() < 1e-9,
        "curr grid_import_cum_wh"
    );
    assert!(
        (curr_readings.grid_export_cum_wh - CURR_EXPORT).abs() < 1e-9,
        "curr grid_export_cum_wh"
    );

    let window_start: i64 = 1746057600;

    // Construct CumulativeReadings from parsed values (as run_raw does from raw_meters_json).
    // Use a synthetic captured_at; the timestamp only affects logging, not delta math.
    let prev_r = CumulativeReading {
        timestamp: window_start - 900 + 1,
        production_wh: prev_readings.production_cum_wh,
        grid_import_cum_wh: prev_readings.grid_import_cum_wh,
        grid_export_cum_wh: prev_readings.grid_export_cum_wh,
    };
    let curr_r = CumulativeReading {
        timestamp: window_start + 1,
        production_wh: curr_readings.production_cum_wh,
        grid_import_cum_wh: curr_readings.grid_import_cum_wh,
        grid_export_cum_wh: curr_readings.grid_export_cum_wh,
    };

    let delta = compute_delta(window_start, &prev_r, &curr_r, true);

    // Results must match the typed test — same inputs, same formula, same output.
    assert!(
        (delta.wh_produced - EXPECTED_WH_PRODUCED).abs() < 1e-9,
        "wh_produced: expected {EXPECTED_WH_PRODUCED}, got {}",
        delta.wh_produced
    );
    assert!(
        (delta.wh_grid_import - EXPECTED_WH_IMPORT).abs() < 1e-9,
        "wh_grid_import: expected {EXPECTED_WH_IMPORT}, got {}",
        delta.wh_grid_import
    );
    assert!(
        (delta.wh_grid_export - EXPECTED_WH_EXPORT).abs() < 1e-9,
        "wh_grid_export: expected {EXPECTED_WH_EXPORT}, got {}",
        delta.wh_grid_export
    );
    assert!(
        (delta.wh_consumed - EXPECTED_WH_CONSUMED).abs() < 1e-9,
        "wh_consumed: expected {EXPECTED_WH_CONSUMED}, got {}",
        delta.wh_consumed
    );
}

/// 11.15 (FormulaFilter::Recomputable excludes formula_version=0) — `query_range` with
/// `FormulaFilter::Recomputable` filters out formula_version=0 rows (permanently unrecomputable),
/// keeping only rows with formula_version > 0. This is the guard that `run_raw` relies on to skip
/// unrecomputable rows.
#[tokio::test]
async fn test_formula_filter_recomputable_excludes_version_zero() {
    let pool = test_pool().await;

    // Row A: formula_version=0 (permanently unrecomputable — pre-boundary_snapshot era).
    // Must use window_start < 1746057600 to match the migration UPDATE's intent, or we can just
    // bind formula_version=0 explicitly for any window_start (migration UPDATE is a no-op on
    // an empty table at migration time; subsequent inserts use our explicit value).
    let ts_unrecomputable: i64 = 1704067200; // 2024-01-01 00:00 UTC — well before cutoff
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version)
         VALUES (?, 50.0, 40.0, 5.0, 15.0, 1, 0)",
    )
    .bind(ts_unrecomputable)
    .execute(&pool)
    .await
    .expect("insert unrecomputable row");

    // Row B: formula_version=1 (current — recomputable).
    let ts_recomputable: i64 = 1746057600; // at the cutoff — must use explicit formula_version=1
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version)
         VALUES (?, 200.0, 150.0, 20.0, 70.0, 1, 1)",
    )
    .bind(ts_recomputable)
    .execute(&pool)
    .await
    .expect("insert recomputable row");

    // FormulaFilter::Recomputable must return only the formula_version=1 row.
    let rows =
        energy_window::query_range(&pool, 0, i64::MAX, i32::MAX, 0, FormulaFilter::Recomputable)
            .await
            .expect("query_range Recomputable");

    assert_eq!(
        rows.len(),
        1,
        "FormulaFilter::Recomputable must return exactly one row, got {}",
        rows.len()
    );
    assert_eq!(
        rows[0].window_start, ts_recomputable,
        "the returned row must be the formula_version=1 row"
    );
    assert_eq!(
        rows[0].formula_version, 1,
        "returned row formula_version must be 1"
    );
}

// 6.3 — clamped window has was_clamped = true, unclamped has was_clamped = false;
// count_clamped returns the count of clamped rows only.
#[tokio::test]
async fn test_was_clamped_round_trip_and_count() {
    use enphase_bridge::storage::models::EnergyWindow;

    let pool = test_pool().await;

    let clamped_window = EnergyWindow {
        id: 0,
        window_start: 1704067200,
        wh_produced: 10.0,
        wh_consumed: 0.0,
        wh_grid_import: 5.0,
        wh_grid_export: 100.0,
        is_complete: true,
        formula_version: CURRENT_FORMULA_VERSION,
        was_clamped: true,
        avg_production_w: None,
        avg_consumption_w: None,
        avg_grid_w: None,
    };
    let unclamped_window = EnergyWindow {
        id: 0,
        window_start: 1704068100,
        wh_produced: 100.0,
        wh_consumed: 130.0,
        wh_grid_import: 50.0,
        wh_grid_export: 20.0,
        is_complete: true,
        formula_version: CURRENT_FORMULA_VERSION,
        was_clamped: false,
        avg_production_w: None,
        avg_consumption_w: None,
        avg_grid_w: None,
    };

    energy_window::insert(&pool, &clamped_window)
        .await
        .expect("insert clamped_window");
    energy_window::insert(&pool, &unclamped_window)
        .await
        .expect("insert unclamped_window");

    // count_clamped must return 1.
    let n = energy_window::count_clamped(&pool)
        .await
        .expect("count_clamped");
    assert_eq!(n, 1, "count_clamped must return 1, got {n}");

    // query_latest returns the most recent window (window_start = 1704068100, unclamped).
    let latest = energy_window::query_latest(&pool)
        .await
        .expect("query_latest")
        .expect("must have a latest row");
    assert_eq!(
        latest.window_start, 1704068100,
        "query_latest must return the most recent window"
    );
    assert!(
        !latest.was_clamped,
        "the latest (unclamped) window must have was_clamped = false"
    );

    // Verify the clamped row also round-trips correctly via query_range.
    let rows = energy_window::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::All)
        .await
        .expect("query_range");
    let clamped_row = rows
        .iter()
        .find(|r| r.window_start == 1704067200)
        .expect("clamped row must be present");
    assert!(
        clamped_row.was_clamped,
        "clamped row must have was_clamped = true"
    );
}

// 6.5 — migration default: row inserted without was_clamped column gets was_clamped = false.
#[tokio::test]
async fn test_migration_default_backfills_was_clamped_false() {
    let pool = test_pool().await;

    // Deliberately omit was_clamped from the INSERT — DEFAULT 0 (migration 003) must apply.
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete)
         VALUES (1704067200, 50.0, 40.0, 5.0, 15.0, 1)",
    )
    .execute(&pool)
    .await
    .expect("raw insert without was_clamped");

    let row = energy_window::query_latest(&pool)
        .await
        .expect("query_latest")
        .expect("row must exist");

    assert!(
        !row.was_clamped,
        "row inserted without was_clamped must default to was_clamped = false (migration DEFAULT 0)"
    );
}
