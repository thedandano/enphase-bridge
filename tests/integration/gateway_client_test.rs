use enphase_bridge::collector::gateway_client::GatewayClient;
use enphase_bridge::error::{AppError, GatewayError};

// Official sample values from Enphase IQ Gateway Local APIs tech brief (Jan 2023).
// EID_CONSUMPTION (704643584) carries both actEnergyDlvd (import) and actEnergyRcvd (export).
const METERS_RESPONSE_WITH_RCVD: &str = r#"[
  {
    "eid": 704643328,
    "activePower": 3200.0,
    "actEnergyDlvd": 12000.0,
    "actEnergyRcvd": 0.0
  },
  {
    "eid": 704643584,
    "activePower": -2800.0,
    "actEnergyDlvd": 48540.732,
    "actEnergyRcvd": 1244797.861
  }
]"#;

// Response with no net-consumption meter (EID 704643584 absent) — must return MissingMeter error.
const METERS_RESPONSE_NO_CONSUMPTION: &str = r#"[
  {
    "eid": 704643328,
    "activePower": 3200.0,
    "actEnergyDlvd": 12000.0,
    "actEnergyRcvd": 0.0
  }
]"#;

// /ivp/meters probe response — both meters present and enabled.
const METERS_PROBE_RESPONSE: &str = r#"[
  {
    "eid": 704643328,
    "measurementType": "production",
    "state": "enabled"
  },
  {
    "eid": 704643584,
    "measurementType": "net-consumption",
    "state": "enabled"
  }
]"#;

#[tokio::test]
async fn test_get_meter_readings_returns_unauthorized_after_reauth_fails() {
    let mut server = mockito::Server::new_async().await;

    let _check_jwt = server
        .mock("POST", "/auth/check_jwt")
        .with_status(200)
        .with_header(
            "set-cookie",
            "sessionId=test-session-abc; Secure; HttpOnly; path=/",
        )
        .with_body("<h2>Valid token.</h2>")
        .create_async()
        .await;

    // Meters always returns 401: first attempt triggers re-auth, retry still fails.
    let _meters = server
        .mock("GET", "/ivp/meters/readings")
        .with_status(401)
        .expect(2)
        .create_async()
        .await;

    let mut client = GatewayClient::new(server.url(), "test-jwt".to_string());
    let result = client.get_meter_readings().await;

    assert!(
        matches!(result, Err(AppError::Gateway(GatewayError::Unauthorized))),
        "expected Unauthorized, got: {result:?}"
    );
}

#[tokio::test]
async fn test_get_meter_readings_parses_grid_export() {
    let mut server = mockito::Server::new_async().await;

    let _meters = server
        .mock("GET", "/ivp/meters/readings")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(METERS_RESPONSE_WITH_RCVD)
        .create_async()
        .await;

    let mut client = GatewayClient::new(server.url(), "test-jwt".to_string());
    let readings = client.get_meter_readings().await.expect("should succeed");

    assert!(
        (readings.grid_export_cum_wh - 1244797.861).abs() < 1e-3,
        "grid_export_cum_wh should be 1244797.861, got {}",
        readings.grid_export_cum_wh
    );
    assert!(
        (readings.grid_import_cum_wh - 48540.732).abs() < 1e-3,
        "grid_import_cum_wh should be 48540.732, got {}",
        readings.grid_import_cum_wh
    );
}

#[tokio::test]
async fn test_get_meter_readings_missing_consumption_returns_error() {
    let mut server = mockito::Server::new_async().await;

    let _meters = server
        .mock("GET", "/ivp/meters/readings")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(METERS_RESPONSE_NO_CONSUMPTION)
        .create_async()
        .await;

    let mut client = GatewayClient::new(server.url(), "test-jwt".to_string());
    let result = client.get_meter_readings().await;

    assert!(
        matches!(
            result,
            Err(AppError::Gateway(GatewayError::MissingMeter(_)))
        ),
        "expected MissingMeter error when EID_CONSUMPTION absent, got: {result:?}"
    );
}

#[tokio::test]
async fn test_probe_meters_succeeds_when_both_meters_present() {
    let mut server = mockito::Server::new_async().await;

    let _probe = server
        .mock("GET", "/ivp/meters")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(METERS_PROBE_RESPONSE)
        .create_async()
        .await;

    let mut client = GatewayClient::new(server.url(), "test-jwt".to_string());
    let result = client.probe_meters().await;

    assert!(
        result.is_ok(),
        "probe_meters should succeed when both meters present, got: {result:?}"
    );
}

#[tokio::test]
async fn test_probe_meters_errors_when_net_consumption_absent() {
    let mut server = mockito::Server::new_async().await;

    let production_only =
        r#"[{"eid": 704643328, "measurementType": "production", "state": "enabled"}]"#;

    let _probe = server
        .mock("GET", "/ivp/meters")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(production_only)
        .create_async()
        .await;

    let mut client = GatewayClient::new(server.url(), "test-jwt".to_string());
    let result = client.probe_meters().await;

    assert!(
        matches!(
            result,
            Err(AppError::Gateway(GatewayError::MissingMeter(_)))
        ),
        "probe_meters should return MissingMeter when net-consumption absent, got: {result:?}"
    );
}
