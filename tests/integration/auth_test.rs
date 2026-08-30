// T015: Integration test for gateway authentication
// Requires a live gateway or a local mock. Set GATEWAY_HOST and GATEWAY_TOKEN env vars.
// Run with: GATEWAY_HOST=192.168.x.x GATEWAY_TOKEN=eyJ... cargo test --test auth_test
//
// This test is skipped automatically when env vars are absent so CI passes without hardware.

use enphase_bridge::collector::gateway_client::GatewayClient;

#[tokio::test]
async fn test_gateway_auth_and_poll() {
    let host = match std::env::var("GATEWAY_HOST") {
        Ok(h) => h,
        Err(_) => {
            eprintln!("GATEWAY_HOST not set — skipping live integration test");
            return;
        }
    };
    let token = match std::env::var("GATEWAY_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            eprintln!("GATEWAY_TOKEN not set — skipping live integration test");
            return;
        }
    };

    let mut client = GatewayClient::new(host, token);
    let result = client.get_meter_readings().await;

    assert!(
        result.is_ok(),
        "gateway_client.get_meter_readings() failed: {:?}",
        result.err()
    );
    let readings = result.unwrap();
    assert!(
        readings.production_w_now >= 0.0,
        "production_w_now should be non-negative"
    );
    assert!(
        readings.consumption_w_now >= 0.0,
        "consumption_w_now must be a non-negative house load — a negative value means the \
         net-consumption meter is being read as load again; got {}",
        readings.consumption_w_now
    );
    assert!(
        (readings.consumption_w_now - readings.production_w_now - readings.grid_w_now).abs() < 1e-6,
        "power balance must close: {} - {} - {} != 0",
        readings.consumption_w_now,
        readings.production_w_now,
        readings.grid_w_now
    );
    // Deliberately no `grid_w_now != 0.0` assertion: a genuine zero-crossing is a valid
    // instantaneous reading, so that would flake against live hardware. The dead-EID
    // regression is pinned deterministically by
    // gateway_client_test::test_dead_net_meter_does_not_zero_grid_w instead.
}
