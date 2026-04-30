## 1. Error types — add MissingMeter variant

- [x] 1.1 Add `GatewayError::MissingMeter(String)` variant to `error.rs` with `IntoResponse` yielding HTTP 502 and event `"required_meter_absent"`

## 2. Gateway Client — startup meter probe

- [x] 2.1 Add `MeterInfo` struct (derive `Deserialize`): fields `eid: u64`, `measurement_type: String` (`#[serde(rename = "measurementType")]`), `state: String`
- [x] 2.2 Add `probe_meters(&mut self) -> Result<(), AppError>` method to `GatewayClient`: GET `/ivp/meters`, find `measurementType == "net-consumption"` with `state == "enabled"`, return `GatewayError::MissingMeter` with list of seen types if absent or disabled; log `info!(event = "meters_discovered")` with discovered EIDs on success

## 3. Gateway Client — fix EID source for cumulative counters

- [x] 3.1 Change `gateway_client.rs` lines for `grid_import_cum_wh` and `grid_export_cum_wh` to read from `cons` (EID_CONSUMPTION `704643584`) instead of `net` (EID_NET `1023410688`)
- [x] 3.2 Replace `net.is_none()` warn block with `cons.is_none()` returning `Err(GatewayError::MissingMeter("net-consumption".to_string()))` — no more `0.0` silent fallback for missing consumption meter
- [x] 3.3 Remove `EID_NET` from the cumulative counter path (keep it only if used for `grid_w_now`); update doc comments on `grid_import_cum_wh` and `grid_export_cum_wh` fields to reference EID_CONSUMPTION

## 4. Scheduler — integrate startup probe

- [x] 4.1 Call `self.gateway.probe_meters().await` in `scheduler.rs` `run()` immediately after `check_jwt()` succeeds; on error, log and halt (same pattern as `check_jwt` failure)

## 5. Integration tests — update fixtures and add probe tests

- [x] 5.1 Update `METERS_RESPONSE_WITH_RCVD` in `gateway_client_test.rs`: move `actEnergyRcvd` value onto EID `704643584` (not `1023410688`); use distinct values matching official sample (e.g. `actEnergyDlvd: 48540.732`, `actEnergyRcvd: 1244797.861`)
- [x] 5.2 Update `test_get_meter_readings_parses_grid_export` assertions to match new fixture values
- [x] 5.3 Rename `METERS_RESPONSE_NO_NET` → `METERS_RESPONSE_NO_CONSUMPTION` (remove EID `704643584`); update `test_get_meter_readings_missing_eid_net_defaults_zero` to verify `GatewayError::MissingMeter` is returned (not zeros)
- [x] 5.4 Add `METERS_PROBE_RESPONSE` constant: JSON with production and net-consumption meter objects (enabled)
- [x] 5.5 Write `test_probe_meters_succeeds_when_both_meters_present`: mock `/ivp/meters` with `METERS_PROBE_RESPONSE`, assert `probe_meters()` returns `Ok(())`
- [x] 5.6 Write `test_probe_meters_errors_when_net_consumption_absent`: mock `/ivp/meters` with production-only response, assert `probe_meters()` returns `Err(GatewayError::MissingMeter(...))`

## 6. Unit tests — regression test for both bugs

- [x] 6.1 Add `test_compute_delta_export_consumed_nonzero_and_distinct_from_produced` to `window_aggregator_test.rs`: prev=(prod=10000, import=200, export=50), curr=(prod=10500, import=200 flat, export=170) → assert `wh_consumed > 0.0` (Bug v1 guard), `(wh_consumed - 380.0).abs() < 1e-6` (exact value), `wh_consumed != wh_produced` (Bug v2 guard), `wh_grid_export ≈ 120.0`, `wh_grid_import == 0.0`

## 7. Documentation

- [x] 7.1 Update `CLAUDE.md` key design decision: replace reference to EID `1023410688` with EID `704643584` (net-consumption meter); add note about startup meter probe via `GET /ivp/meters`
- [x] 7.2 Update `docs/DATA_CONTRACT.md` energy window field table: add a note under `wh_consumed`, `wh_grid_import`, `wh_grid_export` explaining the energy balance formula and that grid counters are sourced from the net-consumption meter's bidirectional `actEnergyDlvd`/`actEnergyRcvd` counters
- [x] 7.3 Note: **no breaking change** — API response shape is unchanged; values become correct. No version tag required.

## 8. Verify

- [x] 8.1 Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` with no warnings
- [x] 8.2 Run `cargo test` — all tests pass
