use axum::body::Body;
use axum::http::{Request, StatusCode};
use enphase_bridge::api::server::{AppState, create_router};
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
        tou_rate_label: String::new(),
    }
}

async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn test_get_windows_empty_list() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert_eq!(j["windows"], serde_json::json!([]));
    assert_eq!(j["total"], 0);
    assert_eq!(j["offset"], 0);
}

#[tokio::test]
async fn test_get_latest_404_when_no_data() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows/latest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let j = json_body(resp).await;
    assert_eq!(j["error"], "not_found");
}

#[tokio::test]
async fn test_get_windows_invalid_range_400() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows?start=2026-01-02T00:00:00Z&end=2026-01-01T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_latest_returns_seeded_window() {
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO energy_window
         (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete)
         VALUES (1704067200, 150.0, 90.0, 0.0, 60.0, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows/latest")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert_eq!(j["window_start"], 1704067200_i64);
    assert!((j["wh_produced"].as_f64().unwrap() - 150.0).abs() < 1e-6);
    assert!((j["wh_grid_export"].as_f64().unwrap() - 60.0).abs() < 1e-6);
    assert!(j["is_complete"].as_bool().unwrap());
}

#[tokio::test]
async fn test_get_windows_returns_seeded_rows_in_range() {
    let pool = test_pool().await;
    for ts in [1704067200_i64, 1704068100, 1704069000] {
        sqlx::query(
            "INSERT INTO energy_window
             (window_start, wh_produced, wh_consumed, wh_grid_import, wh_grid_export, is_complete)
             VALUES (?, 100.0, 80.0, 0.0, 20.0, 1)",
        )
        .bind(ts)
        .execute(&pool)
        .await
        .unwrap();
    }

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows?start=2024-01-01T00:00:00Z&end=2024-12-31T23:59:59Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert_eq!(j["total"], 3);
    assert_eq!(j["windows"].as_array().unwrap().len(), 3);
}
