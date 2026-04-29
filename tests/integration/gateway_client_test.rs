use enphase_bridge::collector::gateway_client::GatewayClient;
use enphase_bridge::error::{AppError, GatewayError};

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
