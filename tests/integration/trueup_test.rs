use axum::body::Body;
use axum::http::{Request, StatusCode};
use enphase_ds::api::server::{AppState, create_router};
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
        tou_rate_label: rate_label.to_string(),
    }
}

async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

// Reuse the same 3-period fixture from calculator_test:
// period 0 = super-off-peak ($0.15), period 1 = off-peak ($0.25), period 2 = peak ($0.40)
// Weekday: hours 0-5→0, 6-15→1, 16-20→2, 21-23→1
fn fixture_rate_json() -> String {
    let weekday_row = "[0,0,0,0,0,0,1,1,1,1,1,1,1,1,1,1,2,2,2,2,2,1,1,1]";
    let weekend_row = "[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]";
    let months: Vec<&str> = vec![weekday_row; 12];
    let weekend_months: Vec<&str> = vec![weekend_row; 12];
    format!(
        r#"{{"energyweekdayschedule":[{w}],"energyweekendschedule":[{e}],"energyratestructure":[[{{"rate":0.15,"unit":"kWh"}}],[{{"rate":0.25,"unit":"kWh"}}],[{{"rate":0.40,"unit":"kWh"}}]]}}"#,
        w = months.join(","),
        e = weekend_months.join(","),
    )
}

// UTC 2024-01-02 00:00:00 = PST 2024-01-01 16:00 (Monday) → peak
const PEAK_TS: i64 = 1704153600;
// UTC 2024-01-02 08:00:00 = PST 2024-01-02 00:00 (Tuesday) → super-off-peak
const SUPER_OP_TS: i64 = 1704182400;
// UTC 2024-01-02 20:00:00 = PST 2024-01-02 12:00 (Tuesday) → off-peak
const OFF_PEAK_TS: i64 = 1704225600;

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

async fn seed_window(pool: &SqlitePool, window_start: i64, import_wh: f64, export_wh: f64) {
    sqlx::query(
        "INSERT INTO energy_window (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete)
         VALUES (?, 0.0, 0.0, ?, ?, 1)",
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
    assert!((j["net_cost_usd"].as_f64().unwrap() - 0.08).abs() < 0.01);
    assert!((j["breakdown"]["peak"]["import_cost_usd"].as_f64().unwrap() - 0.20).abs() < 0.01);
    assert!(
        (j["breakdown"]["super_off_peak"]["export_credit_usd"]
            .as_f64()
            .unwrap()
            - 0.05)
            .abs()
            < 0.01
    );
    assert_eq!(j["tou_schedule"]["rate_label"], "TestRate");
}
