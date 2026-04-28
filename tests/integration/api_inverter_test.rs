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
        tou_utility_eia_id: 0,
        tou_rate_label: String::new(),
    }
}

async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn seed_snapshot(
    pool: &SqlitePool,
    window_start: i64,
    serial: &str,
    watts: f64,
    online: bool,
) {
    sqlx::query(
        "INSERT INTO microinverter_snapshot (window_start, serial_number, watts_output, is_online)
         VALUES (?, ?, ?, ?)",
    )
    .bind(window_start)
    .bind(serial)
    .bind(watts)
    .bind(online)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn test_get_snapshots_by_window_404_when_empty() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/inverters/snapshots/window/1704067200")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_snapshots_by_window_returns_all_inverters() {
    let pool = test_pool().await;
    seed_snapshot(&pool, 1704067200, "SN001", 250.0, true).await;
    seed_snapshot(&pool, 1704067200, "SN002", 0.0, false).await;
    seed_snapshot(&pool, 1704067200, "SN003", 299.0, true).await;

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/inverters/snapshots/window/1704067200")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    assert_eq!(j["window_start"], 1704067200_i64);
    let inverters = j["inverters"].as_array().unwrap();
    assert_eq!(inverters.len(), 3);
    // Sorted by serial_number ASC
    assert_eq!(inverters[0]["serial_number"], "SN001");
    assert!(inverters[0]["is_online"].as_bool().unwrap());
    assert!(!inverters[1]["is_online"].as_bool().unwrap());
}

#[tokio::test]
async fn test_get_snapshots_with_serial_filter() {
    let pool = test_pool().await;
    seed_snapshot(&pool, 1704067200, "SN001", 250.0, true).await;
    seed_snapshot(&pool, 1704067200, "SN002", 230.0, true).await;
    seed_snapshot(&pool, 1704068100, "SN001", 260.0, true).await;

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/inverters/snapshots?serial=SN001&start=2024-01-01T00:00:00Z&end=2024-12-31T23:59:59Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    // Only SN001 rows
    assert_eq!(j["total"], 2);
    let snaps = j["snapshots"].as_array().unwrap();
    assert!(snaps.iter().all(|s| s["serial_number"] == "SN001"));
}
