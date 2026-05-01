use super::common::{
    OFF_PEAK_TS, PEAK_TS, SUMMER_PEAK_TS, SUMMER_SUPER_OP_TS, SUPER_OP_TS, fixture_rate_json,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use enphase_bridge::api::server::{AppState, create_router};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tower::ServiceExt;

async fn setup_pool() -> SqlitePool {
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

fn make_state(pool: SqlitePool, rate_label: &str) -> AppState {
    AppState {
        pool,
        token_expires_at: 9_999_999_999,
        started_at: 0,
        arrays: Default::default(),
        tou_api_key: String::new(),
        tou_utility_eia_id: 0,
        tou_rate_label: rate_label.to_string(),
        tou_openei_base_url: String::new(),
    }
}

async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn seed_schedule(pool: &SqlitePool, rate_label: &str) -> i64 {
    let result = sqlx::query(
        "INSERT INTO tou_rate_schedule (fetched_at, effective_date, utility_name, rate_label, rate_json)
         VALUES (?, NULL, 'Test Utility', ?, ?)",
    )
    .bind(1_000_000_i64)
    .bind(rate_label)
    .bind(fixture_rate_json())
    .execute(pool)
    .await
    .unwrap();
    result.last_insert_rowid()
}

async fn seed_coastal_schedule(pool: &SqlitePool) -> i64 {
    let rate_json =
        std::fs::read_to_string("tests/fixtures/sdge_tou_dr_coastal_baseline_item.json")
            .expect("coastal baseline fixture must exist");
    let result = sqlx::query(
        "INSERT INTO tou_rate_schedule (fetched_at, effective_date, utility_name, rate_label, rate_json)
         VALUES (?, NULL, 'San Diego Gas & Electric', 'TOU-DR Coastal Baseline Region', ?)",
    )
    .bind(1_000_000_i64)
    .bind(rate_json)
    .execute(pool)
    .await
    .unwrap();
    result.last_insert_rowid()
}

async fn seed_window(pool: &SqlitePool, window_start: i64, import_wh: f64, export_wh: f64) {
    sqlx::query(
        "INSERT INTO energy_window (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version)
         VALUES (?, 0.0, 0.0, ?, ?, 1, 1)",
    )
    .bind(window_start)
    .bind(import_wh)
    .bind(export_wh)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn test_estimate_422_when_no_schedule() {
    let pool = setup_pool().await;
    let app = create_router(make_state(pool, "SomeRate"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-01-01T00:00:00Z&end=2024-01-03T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let j = json_body(resp).await;
    assert_eq!(j["error"], "no_tou_schedule");
}

#[tokio::test]
async fn test_estimate_400_when_params_missing() {
    let pool = setup_pool().await;
    let app = create_router(make_state(pool, "SomeRate"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-01-01T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_estimate_422_when_no_energy_data() {
    let pool = setup_pool().await;
    seed_schedule(&pool, "TestRate").await;
    let app = create_router(make_state(pool, "TestRate"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-01-01T00:00:00Z&end=2024-01-03T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let j = json_body(resp).await;
    assert_eq!(j["error"], "insufficient_data");
}

#[tokio::test]
async fn test_estimate_returns_correct_net_cost() {
    let pool = setup_pool().await;
    seed_schedule(&pool, "TestRate").await;
    // peak: 500 Wh import → $0.20; super-op: 300 Wh export → -$0.045; off-peak: 200 import + 500 export → $0.05 - $0.125
    // net = 0.20 + 0.05 - 0.045 - 0.125 = 0.08
    seed_window(&pool, PEAK_TS, 500.0, 0.0).await;
    seed_window(&pool, SUPER_OP_TS, 0.0, 300.0).await;
    seed_window(&pool, OFF_PEAK_TS, 200.0, 500.0).await;

    let app = create_router(make_state(pool, "TestRate"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-01-01T00:00:00Z&end=2024-01-03T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert!((j["net_cost_usd"].as_f64().unwrap() - 0.08).abs() < 1e-6);
    assert!((j["breakdown"]["peak"]["import_cost_usd"].as_f64().unwrap() - 0.20).abs() < 1e-6);
    assert!(
        (j["breakdown"]["super_off_peak"]["export_credit_usd"]
            .as_f64()
            .unwrap()
            - 0.05)
            .abs()
            < 1e-6
    );
    assert_eq!(j["tou_schedule"]["rate_label"], "TestRate");
}

#[tokio::test]
async fn test_estimate_includes_window_on_end_date() {
    // Window at exact UTC midnight of the end date should be included (+1-day fix).
    let pool = setup_pool().await;
    seed_schedule(&pool, "TestRate").await;

    // end date = 2024-01-03T00:00:00Z; seed a window exactly at that midnight
    let end_midnight: i64 = 1704240000; // 2024-01-03 00:00:00 UTC
    seed_window(&pool, end_midnight, 400.0, 0.0).await;

    let app = create_router(make_state(pool, "TestRate"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-01-01T00:00:00Z&end=2024-01-03T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    // 400 Wh import at off-peak (PST 2024-01-02 16:00 = period 1, $0.25): 0.4 * 0.25 = $0.10
    // The window at end_midnight (UTC midnight 2024-01-03) falls in PST 2024-01-02 16:00 → peak
    assert!(j["breakdown"]["peak"]["import_kwh"].as_f64().unwrap() > 0.0);
}

// 4.1 — 6-period coastal baseline: summer windows show Peak and SuperOffPeak in API response
#[tokio::test]
async fn test_estimate_6period_seasonal_peak_and_super_off_peak() {
    let pool = setup_pool().await;
    seed_coastal_schedule(&pool).await;
    // Summer peak: Jul 1 16:00 PDT → period 0 → Peak
    seed_window(&pool, SUMMER_PEAK_TS, 400.0, 0.0).await;
    // Summer super-off-peak: Jul 2 00:00 PDT → period 2 → SuperOffPeak
    seed_window(&pool, SUMMER_SUPER_OP_TS, 0.0, 200.0).await;

    let app = create_router(make_state(pool, "TOU-DR Coastal Baseline Region"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-07-01T00:00:00Z&end=2024-07-03T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;

    let peak = &j["breakdown"]["peak"];
    let super_off_peak = &j["breakdown"]["super_off_peak"];
    let off_peak = &j["breakdown"]["off_peak"];

    assert!(
        peak["import_kwh"].as_f64().unwrap() > 0.0,
        "summer hour 16 should produce Peak kWh"
    );
    assert!(
        super_off_peak["export_kwh"].as_f64().unwrap() > 0.0,
        "summer hour 0 should produce SuperOffPeak export kWh"
    );
    assert_eq!(peak["export_kwh"].as_f64().unwrap(), 0.0);

    // No off-peak windows were seeded
    assert_eq!(
        off_peak["import_kwh"].as_f64().unwrap(),
        0.0,
        "off_peak import_kwh should be 0"
    );
    assert_eq!(
        off_peak["export_kwh"].as_f64().unwrap(),
        0.0,
        "off_peak export_kwh should be 0"
    );

    // net cost: 400 Wh import at rate 0.60 = $0.24; 200 Wh export at sell 0.30 = $0.06 credit
    // net = 0.24 - 0.06 = 0.18
    assert!(
        (j["net_cost_usd"].as_f64().unwrap() - 0.18).abs() < 1e-6,
        "net_cost_usd expected 0.18, got {}",
        j["net_cost_usd"]
    );

    assert_eq!(
        j["tou_schedule"]["rate_label"],
        "TOU-DR Coastal Baseline Region"
    );
}

#[tokio::test]
async fn test_estimate_same_day_start_end_returns_200() {
    // start == end should be accepted; after +86400 normalization the range covers 2024-01-01.
    let pool = setup_pool().await;
    seed_schedule(&pool, "TestRate").await;
    // 2024-01-01 00:00:00 UTC = 2023-12-31 16:00 PST → peak (period 2)
    seed_window(&pool, 1704067200, 500.0, 0.0).await;

    let app = create_router(make_state(pool, "TestRate"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-01-01T00:00:00Z&end=2024-01-01T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// 11.10: trueup estimate excludes non-current formula_version windows and reports excluded_window_count
#[tokio::test]
async fn test_estimate_excludes_non_current_formula_windows() {
    let pool = setup_pool().await;
    seed_schedule(&pool, "TestRate").await;

    // Seed 3 windows with formula_version=0 (unrecomputable) at timestamps within the range
    // Use timestamps that don't collide with PEAK_TS or OFF_PEAK_TS
    for ts in [1704067200_i64, 1704068100, 1704069000] {
        sqlx::query(
            "INSERT INTO energy_window (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version)
             VALUES (?, 0.0, 0.0, 0.0, 0.0, 1, 0)",
        )
        .bind(ts)
        .execute(&pool)
        .await
        .unwrap();
    }

    // Seed 2 windows using the helper (formula_version=1 = current)
    seed_window(&pool, PEAK_TS, 500.0, 0.0).await;
    seed_window(&pool, OFF_PEAK_TS, 0.0, 200.0).await;

    let app = create_router(make_state(pool, "TestRate"));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/trueup/estimate?start=2024-01-01T00:00:00Z&end=2024-01-03T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert_eq!(
        j["excluded_window_count"].as_u64().unwrap(),
        3,
        "3 unrecomputable windows should be excluded from the estimate"
    );
}
