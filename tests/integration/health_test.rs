use axum::body::Body;
use axum::http::{Request, StatusCode};
use enphase_bridge::api::server::{AppState, create_router};
use enphase_bridge::collector::window_aggregator::CURRENT_FORMULA_VERSION;
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

async fn seed_window_with_version(pool: &SqlitePool, window_start: i64, formula_version: i32) {
    sqlx::query(
        "INSERT INTO energy_window (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version)
         VALUES (?, 0.0, 0.0, 0.0, 0.0, 1, ?)",
    )
    .bind(window_start)
    .bind(formula_version)
    .execute(pool)
    .await
    .unwrap();
}

// 11.9(a): health endpoint counts unrecomputable (formula_version=0) and stale windows correctly
#[tokio::test]
async fn test_health_counts_unrecomputable_and_stale() {
    let pool = test_pool().await;

    // Seed 2 rows with formula_version=0 (unrecomputable)
    seed_window_with_version(&pool, 1704067200, 0).await;
    seed_window_with_version(&pool, 1704068100, 0).await;

    // No stale rows possible when CURRENT_FORMULA_VERSION=1 (no version between 0 and 1 exclusive)

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;

    assert!(
        j["unrecomputable_window_count"].is_number(),
        "unrecomputable_window_count should be a number, got: {}",
        j["unrecomputable_window_count"]
    );
    assert_eq!(
        j["unrecomputable_window_count"].as_i64().unwrap(),
        2,
        "expected 2 unrecomputable windows"
    );
    assert!(
        j["stale_window_count"].is_number(),
        "stale_window_count should be a number, got: {}",
        j["stale_window_count"]
    );
    assert_eq!(
        j["stale_window_count"].as_i64().unwrap(),
        0,
        "expected 0 stale windows (no version between 0 and CURRENT exclusive)"
    );
}

// 6.4 — GET /api/health returns clamped_window_count equal to the number of was_clamped rows.
#[tokio::test]
async fn test_health_clamped_window_count() {
    let pool = test_pool().await;

    // Insert one clamped row (was_clamped = 1) and one unclamped row (was_clamped = 0).
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped)
         VALUES (1704067200, 10.0, 0.0, 5.0, 100.0, 1, 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete, formula_version, was_clamped)
         VALUES (1704068100, 100.0, 130.0, 50.0, 20.0, 1, 1, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;

    assert!(
        j["clamped_window_count"].is_number(),
        "clamped_window_count must be a number in health response, got: {}",
        j["clamped_window_count"]
    );
    assert_eq!(
        j["clamped_window_count"].as_i64().unwrap(),
        1,
        "clamped_window_count must equal 1 (one was_clamped row inserted)"
    );
}

// 11.9(b): health endpoint reports zero counts when all rows are current
#[tokio::test]
async fn test_health_zero_counts_with_only_current_rows() {
    let pool = test_pool().await;

    // Seed 1 row with formula_version=CURRENT_FORMULA_VERSION
    seed_window_with_version(&pool, 1704067200, CURRENT_FORMULA_VERSION).await;

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;

    assert!(
        j["unrecomputable_window_count"].is_number(),
        "unrecomputable_window_count should be a number, got: {}",
        j["unrecomputable_window_count"]
    );
    assert_eq!(
        j["unrecomputable_window_count"].as_i64().unwrap(),
        0,
        "expected 0 unrecomputable windows"
    );
    assert!(
        j["stale_window_count"].is_number(),
        "stale_window_count should be a number, got: {}",
        j["stale_window_count"]
    );
    assert_eq!(
        j["stale_window_count"].as_i64().unwrap(),
        0,
        "expected 0 stale windows"
    );
}
