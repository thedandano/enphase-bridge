## Why

EID `1023410688` (used as the grid meter source in the previous fix) is not a documented Enphase IQ Gateway meter EID — the official Enphase Local APIs tech brief (Jan 2023) defines exactly two top-level meters: production (`704643328`) and net-consumption (`704643584`). Every poll returns `None` for this phantom EID, silently pinning both grid counters to `0.0` and causing `wh_consumed = wh_produced` in all windows.

## What Changes

- Switch `gateway_client.rs` to read `actEnergyDlvd` (grid import) and `actEnergyRcvd` (grid export) from EID_CONSUMPTION (`704643584`) — the documented `net-consumption` meter whose bidirectional counters are confirmed live (`actEnergyRcvd: 1,244,797 Wh` in the official sample).
- Add a `GET /ivp/meters` startup probe that discovers and validates meter presence by `measurementType`; fail loudly if the net-consumption meter is absent rather than silently defaulting to zeros.
- Elevate the missing-meter condition from `warn!` with `0.0` fallback to `error!` / typed `GatewayError`, enforcing the project's "no silent failures" rule.
- Update integration test fixtures: `actEnergyRcvd` must be on EID_CONSUMPTION (`704643584`), not EID_NET (`1023410688`).
- Add regression tests asserting `wh_consumed > 0` AND `wh_consumed != wh_produced` for export-window scenarios.
- Update `CLAUDE.md` key design decision to reflect the corrected source EID.
- Update `docs/DATA_CONTRACT.md` to document the energy balance methodology and note the corrected data source.

No API response shape change. Values become correct for the first time. No new config fields required.

## Capabilities

### New Capabilities

- `gateway-meter-discovery`: Startup probe that calls `GET /ivp/meters`, maps `measurementType → eid`, validates required meters are enabled, and fails fast with a clear error when the net-consumption CT is absent.

### Modified Capabilities

- `energy-metering`: Grid import counter (`wh_grid_import`) is sourced from `actEnergyDlvd` on the net-consumption meter (`704643584`); grid export counter (`wh_grid_export`) is sourced from `actEnergyRcvd` on the same meter. Consumption is still derived from the energy balance (`produced + grid_import - grid_export`). The fix corrects the source EID; the formula is unchanged.

## Impact

- **`src/collector/gateway_client.rs`**: Remove `EID_NET` cumulative usage; read both counters from EID_CONSUMPTION; add `/ivp/meters` probe at startup; elevate absence error.
- **`tests/integration/gateway_client_test.rs`**: Update `METERS_RESPONSE_WITH_RCVD` to place `actEnergyRcvd` on EID_CONSUMPTION; update related assertions.
- **`tests/unit/window_aggregator_test.rs`**: Add `test_compute_delta_export_consumed_nonzero_and_distinct_from_produced` regression test.
- **`CLAUDE.md`**: Update key design decision for the corrected source EID.
- **`docs/DATA_CONTRACT.md`**: Add methodology note to the energy window field descriptions.
- **No DB schema changes.** **No API contract shape changes.** Values become more accurate, not structurally different.
