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
        "eiaid": 12345,
        "label": format!("revision-{startdate}"),
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
    let history = client.fetch_history().await.expect("fetch should succeed");
    let fetched = history.last().unwrap();

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
    let err = client.fetch_history().await.unwrap_err();
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
    let history = client.fetch_history().await.expect("fetch should succeed");
    let fetched = history.last().unwrap();
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
    let err = client.fetch_history().await.unwrap_err();
    assert!(
        matches!(err, AppError::Tou(TouError::UpstreamUnavailable(_))),
        "expected UpstreamUnavailable, got: {err:?}"
    );
}

#[tokio::test]
async fn fetch_history_keeps_all_revisions_and_pages() {
    let mut server = mockito::Server::new_async().await;
    let mut page = vec![minimal_item("Other", 1); 500];
    page[0] = minimal_item("My Rate", 1600000000);
    let first = server
        .mock("GET", mockito::Matcher::Regex("offset=0".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!(page)))
        .create_async()
        .await;
    let second = server
        .mock("GET", mockito::Matcher::Regex("offset=500".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!([minimal_item(
            "My Rate", 1700000000
        )])))
        .create_async()
        .await;
    let fetched = make_client(&server.url(), "My Rate")
        .fetch_history()
        .await
        .unwrap();
    assert_eq!(fetched.len(), 2);
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn malformed_revision_bounds_are_archived_without_blocking_valid_history() {
    let mut server = mockito::Server::new_async().await;
    let mut invalid = minimal_item("My Rate", 100);
    invalid["enddate"] = serde_json::json!(99);
    let mock = server
        .mock("GET", mockito::Matcher::Regex("^/utility_rates".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!([
            invalid,
            minimal_item("My Rate", 200)
        ])))
        .create_async()
        .await;
    let history = make_client(&server.url(), "My Rate")
        .fetch_history()
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].effective_end, Some(99));
    mock.assert_async().await;
}

#[tokio::test]
async fn repeated_full_page_fails_instead_of_looping_forever() {
    let mut server = mockito::Server::new_async().await;
    let page = vec![minimal_item("My Rate", 1); 500];
    let mock = server
        .mock("GET", mockito::Matcher::Regex("^/utility_rates".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!(page)))
        .expect_at_least(2)
        .create_async()
        .await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        make_client(&server.url(), "My Rate").fetch_history(),
    )
    .await;
    assert!(result.is_ok(), "pagination must stop on a repeated page");
    assert!(matches!(
        result.unwrap(),
        Err(AppError::Tou(TouError::ParseError(_)))
    ));
    mock.assert_async().await;
}

#[tokio::test]
async fn missing_utility_identity_is_rejected() {
    let mut server = mockito::Server::new_async().await;
    let mut item = minimal_item("My Rate", 1);
    item.as_object_mut().unwrap().remove("eiaid");
    let mock = server
        .mock("GET", mockito::Matcher::Regex("^/utility_rates".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(items_response(serde_json::json!([item])))
        .create_async()
        .await;
    assert!(matches!(
        make_client(&server.url(), "My Rate").fetch_history().await,
        Err(AppError::Tou(TouError::ParseError(_)))
    ));
    mock.assert_async().await;
}
