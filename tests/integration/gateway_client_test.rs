use enphase_bridge::collector::gateway_client::GatewayClient;
use enphase_bridge::error::{AppError, GatewayError};

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
    "actEnergyDlvd": 9500.0,
    "actEnergyRcvd": 0.0
  },
  {
    "eid": 1023410688,
    "activePower": 400.0,
    "actEnergyDlvd": 1500.0,
    "actEnergyRcvd": 750.0
  }
]"#;

const METERS_RESPONSE_NO_NET: &str = r#"[
  {
    "eid": 704643328,
    "activePower": 3200.0,
    "actEnergyDlvd": 12000.0,
    "actEnergyRcvd": 0.0
  },
  {
    "eid": 704643584,
    "activePower": -2800.0,
    "actEnergyDlvd": 9500.0,
    "actEnergyRcvd": 0.0
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
        (readings.grid_export_cum_wh - 750.0).abs() < 1e-6,
        "grid_export_cum_wh should be 750.0, got {}",
        readings.grid_export_cum_wh
    );
    assert!(
        (readings.grid_import_cum_wh - 1500.0).abs() < 1e-6,
        "grid_import_cum_wh should be 1500.0, got {}",
        readings.grid_import_cum_wh
    );
}

#[tokio::test]
async fn test_get_meter_readings_missing_eid_net_defaults_zero() {
    let mut server = mockito::Server::new_async().await;

    let _meters = server
        .mock("GET", "/ivp/meters/readings")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(METERS_RESPONSE_NO_NET)
        .create_async()
        .await;

    let mut client = GatewayClient::new(server.url(), "test-jwt".to_string());
    let readings = client.get_meter_readings().await.expect("should succeed");

    assert!(
        (readings.grid_import_cum_wh - 0.0).abs() < 1e-6,
        "grid_import_cum_wh should be 0.0 when EID_NET absent, got {}",
        readings.grid_import_cum_wh
    );
    assert!(
        (readings.grid_export_cum_wh - 0.0).abs() < 1e-6,
        "grid_export_cum_wh should be 0.0 when EID_NET absent, got {}",
        readings.grid_export_cum_wh
    );
}
