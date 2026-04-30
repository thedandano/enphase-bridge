use enphase_bridge::error::{AppError, TouError};
use enphase_bridge::tou::openei_client::OpenEiClient;

fn make_client(server_url: &str, rate_label: &str) -> OpenEiClient {
    OpenEiClient::with_base_url(
        "test-api-key".to_string(),
        12345,
        rate_label.to_string(),
        server_url.to_string(),
    )
}

fn items_response(items: serde_json::Value) -> String {
    serde_json::json!({ "items": items }).to_string()
}

fn minimal_item(name: &str, startdate: i64) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "utility": "Test Utility",
        "startdate": startdate,
        "energyratestructure": [[{"rate": 0.25, "sell": 0.25, "unit": "kWh"}]],
        "energyweekdayschedule": vec![vec![0_i32; 24]; 12],
        "energyweekendschedule": vec![vec![0_i32; 24]; 12],
    })
}

// 11.2 — successful fetch returns correct fields
#[tokio::test]
async fn test_fetch_success_returns_correct_fields() {
    let mut server = mockito::Server::new_async().await;
    let item = minimal_item("My Rate", 1700000000);
    let _mock = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/utility_rates".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!([item])))
        .create_async()
        .await;

    let client = make_client(&server.url(), "My Rate");
    let fetched = client.fetch().await.expect("fetch should succeed");

    assert_eq!(fetched.rate_label, "My Rate");
    assert_eq!(fetched.utility_name, "Test Utility");
    assert!(!fetched.rate_json.is_empty());
}

// 11.3 — label not found → ParseError
#[tokio::test]
async fn test_fetch_label_not_found_returns_parse_error() {
    let mut server = mockito::Server::new_async().await;
    let item = minimal_item("Other Rate", 1700000000);
    let _mock = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/utility_rates".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!([item])))
        .create_async()
        .await;

    let client = make_client(&server.url(), "Not Found Rate");
    let err = client.fetch().await.unwrap_err();
    assert!(
        matches!(err, AppError::Tou(TouError::ParseError(_))),
        "expected ParseError, got: {err:?}"
    );
}

// 11.4 — multiple items with same name, different startdate → most recent selected
#[tokio::test]
async fn test_fetch_picks_most_recent_when_duplicates() {
    let mut server = mockito::Server::new_async().await;
    let old_item = minimal_item("My Rate", 1600000000);
    let new_item = {
        let mut i = minimal_item("My Rate", 1700000000);
        i["utility"] = serde_json::json!("Newer Utility");
        i
    };
    let _mock = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/utility_rates".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!([old_item, new_item])))
        .create_async()
        .await;

    let client = make_client(&server.url(), "My Rate");
    let fetched = client.fetch().await.expect("fetch should succeed");
    assert_eq!(fetched.utility_name, "Newer Utility");
}

// 11.5 — non-200 response → UpstreamUnavailable
#[tokio::test]
async fn test_fetch_non_200_returns_upstream_unavailable() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/utility_rates".to_string()),
        )
        .with_status(503)
        .create_async()
        .await;

    let client = make_client(&server.url(), "My Rate");
    let err = client.fetch().await.unwrap_err();
    assert!(
        matches!(err, AppError::Tou(TouError::UpstreamUnavailable(_))),
        "expected UpstreamUnavailable, got: {err:?}"
    );
}
