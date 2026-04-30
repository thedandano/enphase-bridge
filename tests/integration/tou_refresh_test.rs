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

fn make_state(pool: SqlitePool, base_url: &str) -> AppState {
    AppState {
        pool,
        token_expires_at: 9_999_999_999,
        started_at: 0,
        arrays: Default::default(),
        tou_api_key: "test-key".to_string(),
        tou_utility_eia_id: 12345,
        tou_rate_label: "TOU-DR-2".to_string(),
        tou_openei_base_url: base_url.to_string(),
    }
}

async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn openei_response_body() -> String {
    let item = serde_json::json!({
        "name": "TOU-DR-2",
        "utility": "San Diego Gas & Electric",
        "startdate": 1700000000_i64,
        "energyratestructure": [[{"rate": 0.40, "sell": 0.40, "unit": "kWh"}]],
        "energyweekdayschedule": vec![vec![0_i32; 24]; 12],
        "energyweekendschedule": vec![vec![0_i32; 24]; 12],
    });
    serde_json::json!({ "items": [item] }).to_string()
}

// 12.1 — POST /api/tou/refresh returns 200 with valid schedule_id
// 12.2 — inserted row has correct rate_label and non-empty rate_json
#[tokio::test]
async fn test_post_tou_refresh_inserts_schedule() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/utility_rates".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(openei_response_body())
        .create_async()
        .await;

    let pool = setup_pool().await;
    let app = create_router(make_state(pool.clone(), &server.url()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tou/refresh")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;
    let schedule_id = j["schedule_id"]
        .as_i64()
        .expect("schedule_id must be present");
    assert!(schedule_id > 0);

    // Verify DB row
    let row: (String, String) =
        sqlx::query_as("SELECT rate_label, rate_json FROM tou_rate_schedule WHERE id = ?")
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .expect("row must exist");
    assert_eq!(row.0, "TOU-DR-2");
    assert!(!row.1.is_empty());
}
