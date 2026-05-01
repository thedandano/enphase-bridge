use axum::body::Body;
use axum::http::{Request, StatusCode};
use enphase_bridge::api::server::{AppState, create_router};
use enphase_bridge::storage::models::PhaseReading;
use enphase_bridge::storage::phase_reading;
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

fn make_reading(sampled_at: i64, meter_eid: i64, channel_eid: i64) -> PhaseReading {
    PhaseReading {
        id: 0,
        sampled_at,
        meter_eid,
        channel_eid,
        active_power_w_at_boundary: 500.0,
        energy_dlvd_wh: 10000.0,
        energy_rcvd_wh: 0.0,
    }
}

/// 7.5a — GET /api/power/phases returns all rows in the requested time range.
#[tokio::test]
async fn test_get_phases_returns_rows() {
    let pool = test_pool().await;
    let rows = vec![
        make_reading(1000, 704643328, 1778385169),
        make_reading(2000, 704643328, 1778385169),
    ];
    phase_reading::insert_batch(&pool, &rows).await.unwrap();

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/power/phases?start=500&end=2500")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = json_body(resp).await;
    assert_eq!(json["total"], 2, "should return 2 phase readings in range");
    assert_eq!(json["readings"].as_array().unwrap().len(), 2);
}

/// 7.5b — GET /api/power/phases?meter_eid=X returns only rows for that meter.
#[tokio::test]
async fn test_get_phases_filter_by_meter_eid() {
    let pool = test_pool().await;
    let rows = vec![
        make_reading(1000, 704643328, 1778385169),
        make_reading(1000, 704643584, 1778385171),
    ];
    phase_reading::insert_batch(&pool, &rows).await.unwrap();

    let app = create_router(make_state(pool));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/power/phases?start=500&end=2000&meter_eid=704643328")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = json_body(resp).await;
    assert_eq!(
        json["total"], 1,
        "filter by meter_eid should return only 1 row"
    );
    let reading = &json["readings"][0];
    assert_eq!(reading["meter_eid"], 704643328);
    assert_eq!(reading["channel_eid"], 1778385169);
}

/// 7.5c — end < start returns HTTP 400 with error = "invalid_param".
#[tokio::test]
async fn test_get_phases_end_before_start_returns_400() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/power/phases?start=2000&end=1000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let json = json_body(resp).await;
    assert_eq!(json["error"], "invalid_param");
}

/// 7.5d — empty range returns an empty readings array with total = 0.
#[tokio::test]
async fn test_get_phases_empty_range_returns_empty() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/power/phases?start=1704067200&end=1704070800")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let json = json_body(resp).await;
    assert_eq!(json["total"], 0);
    assert_eq!(json["readings"], serde_json::json!([]));
}

/// 7.5e — missing start parameter returns HTTP 400.
#[tokio::test]
async fn test_get_phases_missing_start_returns_400() {
    let app = create_router(make_state(test_pool().await));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/power/phases?end=2000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
