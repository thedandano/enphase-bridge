mod common;

#[path = "integration/auth_test.rs"]
mod auth_test;

#[path = "integration/api_energy_test.rs"]
mod api_energy_test;

#[path = "integration/api_inverter_test.rs"]
mod api_inverter_test;

#[path = "integration/trueup_test.rs"]
mod trueup_test;

#[path = "integration/gateway_client_test.rs"]
mod gateway_client_test;

#[path = "integration/tou_refresh_test.rs"]
mod tou_refresh_test;

#[path = "integration/scheduler_test.rs"]
mod scheduler_test;

#[path = "integration/boundary_snapshot_test.rs"]
mod boundary_snapshot_test;

#[path = "integration/health_test.rs"]
mod health_test;

#[path = "integration/recompute_test.rs"]
mod recompute_test;

#[path = "integration/power_sample_test.rs"]
mod power_sample_test;

#[path = "integration/api_power_test.rs"]
mod api_power_test;

#[path = "integration/phase_reading_test.rs"]
mod phase_reading_test;

#[path = "integration/api_phases_test.rs"]
mod api_phases_test;
