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
        tou_timezone: chrono_tz::America::Los_Angeles,
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
        "eiaid": 12345,
        "label": "revision-1",
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

#[tokio::test]
async fn refresh_rolls_back_when_a_later_revision_insert_fails() {
    let mut server = mockito::Server::new_async().await;
    let mut first: serde_json::Value = serde_json::from_str(&openei_response_body()).unwrap();
    let mut second = first["items"][0].clone();
    second["label"] = serde_json::json!("rejected");
    second["startdate"] = serde_json::json!(1800000000);
    first["items"].as_array_mut().unwrap().push(second);
    let mock = server
        .mock("GET", mockito::Matcher::Regex("^/utility_rates".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(first.to_string())
        .create_async()
        .await;
    let pool = setup_pool().await;
    sqlx::query("CREATE TRIGGER reject_revision BEFORE INSERT ON tou_rate_schedule WHEN NEW.source_id = 'rejected' BEGIN SELECT RAISE(ABORT, 'test insert failure'); END")
        .execute(&pool).await.unwrap();
    let response = create_router(make_state(pool.clone(), &server.url()))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tou/refresh")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tou_rate_schedule")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    mock.assert_async().await;
}

#[tokio::test]
async fn migration_preserves_unknown_rows_and_backfills_only_verified_history() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../../migrations/001_initial.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let valid = serde_json::json!({"eiaid":12345,"label":"verified","startdate":100,"enddate":200});
    for json in ["not-json".to_string(), "{}".to_string(), valid.to_string()] {
        sqlx::query("INSERT INTO tou_rate_schedule (fetched_at, utility_name, rate_label, rate_json) VALUES (0, 'Utility', 'Rate', ?)")
            .bind(json).execute(&pool).await.unwrap();
    }
    sqlx::raw_sql(include_str!("../../migrations/007_tou_history.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tou_rate_schedule")
        .fetch_one(&pool)
        .await
        .unwrap();
    let verified: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tou_rate_schedule WHERE utility_eia_id = 12345 AND effective_start = 100 AND effective_end = 200").fetch_one(&pool).await.unwrap();
    assert_eq!(total, 3);
    assert_eq!(verified, 1);
}
