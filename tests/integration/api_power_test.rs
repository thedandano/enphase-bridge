use axum::body::Body;
use axum::http::{Request, StatusCode};
use enphase_bridge::api::server::{AppState, create_router};
use enphase_bridge::collector::window_aggregator::{CumulativeReading, compute_delta};
use enphase_bridge::storage::energy_window::FormulaFilter;
use enphase_bridge::storage::{energy_window as ew_store, power_sample as ps_store};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tower::ServiceExt;

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

fn make_state(pool: SqlitePool) -> AppState {
    AppState {
        pool,
        token_expires_at: 9_999_999_999,
        started_at: 0,
        arrays: Default::default(),
        tou_api_key: String::new(),
        tou_utility_eia_id: 0,
        tou_rate_label: String::new(),
        tou_openei_base_url: String::new(),
    }
}

async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// 6.4 — avg_* fields survive the ew_store::insert → query_range storage round trip.
/// Note: tests the DB persistence contract; the scheduler's in-memory accumulator arithmetic
/// (scheduler.rs lines 91-99) is exercised by real scheduler runs, not directly by this test.
#[tokio::test]
async fn test_avg_fields_db_roundtrip() {
    let pool = test_pool().await;
    let window_start: i64 = 1704067200;

    // Simulate what the scheduler's accumulator would compute after 3 ticks
    // tick 1: prod=2000, cons=1500, grid=-500
    // tick 2: prod=2400, cons=1800, grid=-600
    // tick 3: prod=2200, cons=1600, grid=-550
    // expected averages: prod=2200.0, cons=1633.333..., grid=-550.0
    let samples = [
        (2000.0_f64, 1500.0_f64, -500.0_f64),
        (2400.0, 1800.0, -600.0),
        (2200.0, 1600.0, -550.0),
    ];
    let n = samples.len() as f64;
    let avg_prod = samples.iter().map(|s| s.0).sum::<f64>() / n;
    let avg_cons = samples.iter().map(|s| s.1).sum::<f64>() / n;
    let avg_grid = samples.iter().map(|s| s.2).sum::<f64>() / n;

    // Build EnergyWindow with computed averages (as the scheduler does after accumulator.clear())
    let prev = CumulativeReading {
        timestamp: window_start,
        production_wh: 100_000.0,
        grid_import_cum_wh: 5_000.0,
        grid_export_cum_wh: 1_000.0,
    };
    let curr = CumulativeReading {
        timestamp: window_start + 905,
        production_wh: 100_875.0,
        grid_import_cum_wh: 5_000.0,
        grid_export_cum_wh: 1_120.0,
    };
    let mut window = compute_delta(window_start, &prev, &curr, true);
    window.avg_production_w = Some(avg_prod);
    window.avg_consumption_w = Some(avg_cons);
    window.avg_grid_w = Some(avg_grid);

    ew_store::insert(&pool, &window).await.unwrap();

    let rows = ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::All)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let stored = &rows[0];

    assert!(
        stored.avg_production_w.is_some(),
        "avg_production_w must be stored (not None)"
    );
    assert!(
        (stored.avg_production_w.unwrap() - avg_prod).abs() < 1e-6,
        "avg_production_w mismatch: expected {avg_prod}, got {:?}",
        stored.avg_production_w
    );
    assert!(
        (stored.avg_consumption_w.unwrap() - avg_cons).abs() < 1e-6,
        "avg_consumption_w mismatch"
    );
    assert!(
        (stored.avg_grid_w.unwrap() - avg_grid).abs() < 1e-6,
        "avg_grid_w mismatch"
    );
}

/// 6.5a — GET /api/power/samples?start=T&end=T+3600 returns correct rows.
#[tokio::test]
async fn test_get_power_samples_returns_rows_in_range() {
    let pool = test_pool().await;
    let t_start: i64 = 1704067200;
    let t_end: i64 = t_start + 3600;

    // Insert 2 samples in range and 1 outside
    ps_store::insert(&pool, t_start + 60, 2000.0, 1500.0, -500.0)
        .await
        .unwrap();
    ps_store::insert(&pool, t_start + 120, 2100.0, 1600.0, -600.0)
        .await
        .unwrap();
    ps_store::insert(&pool, t_end + 60, 999.0, 999.0, 999.0)
        .await
        .unwrap(); // outside range

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/power/samples?start={t_start}&end={t_end}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert_eq!(j["total"], 2, "should return 2 samples in range");
    assert_eq!(j["samples"].as_array().unwrap().len(), 2);
}

/// 6.5b — empty range returns [].
#[tokio::test]
async fn test_get_power_samples_empty_range_returns_empty_array() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/power/samples?start=1704067200&end=1704070800")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert_eq!(j["total"], 0);
    assert_eq!(j["samples"], serde_json::json!([]));
}

/// 6.5c — end < start returns HTTP 400.
#[tokio::test]
async fn test_get_power_samples_end_before_start_returns_400() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/power/samples?start=1704070800&end=1704067200")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let j = json_body(resp).await;
    assert_eq!(j["error"], "invalid_param");
}

/// 6.6 — historical energy_window rows (inserted without avg columns) have NULL avg fields.
#[tokio::test]
async fn test_historical_energy_window_rows_have_null_avg_columns() {
    let pool = test_pool().await;

    // Insert a "historical" row without specifying avg columns (simulates pre-migration data)
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete)
         VALUES (1704067200, 100.0, 80.0, 0.0, 20.0, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let rows = ew_store::query_range(&pool, 0, i64::MAX, 100, 0, FormulaFilter::All)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!(
        row.avg_production_w.is_none(),
        "historical row avg_production_w must be NULL"
    );
    assert!(
        row.avg_consumption_w.is_none(),
        "historical row avg_consumption_w must be NULL"
    );
    assert!(
        row.avg_grid_w.is_none(),
        "historical row avg_grid_w must be NULL"
    );
}
