## Why

`wh_consumed` drops to zero in windows where the solar array is net-exporting to the grid because the bridge reads `actEnergyDlvd` from the consumption meter as if it measures total house load — but on net-metering setups it only tracks energy delivered *from the grid*, stalling to zero the moment solar covers all consumption and begins exporting.

## What Changes

- Add `actEnergyRcvd` deserialization to `MeterObject` in `gateway_client.rs` so the gateway's grid-export counter is available.
- Expose `grid_export_cum_wh` (from `actEnergyRcvd` on the net meter) alongside `grid_import_cum_wh` in `MeterReadings`.
- Replace `consumption_wh` in `CumulativeReading` with explicit `grid_import_cum_wh` and `grid_export_cum_wh` fields.
- Rewrite `compute_delta` to derive house consumption via energy balance:
  `wh_consumed = wh_produced + wh_grid_import - wh_grid_export`
- Persist the new `grid_export_cum_wh` field in `config_store` so it survives restarts.
- Update all affected unit and integration tests; add a regression test for the zero-consumption-during-export scenario.

## Capabilities

### New Capabilities

- `energy-metering`: Correct bidirectional energy accounting — production, consumption, grid import, and grid export — using the net meter's `actEnergyDlvd` and `actEnergyRcvd` fields, with consumption derived from the energy balance so it remains accurate during solar export windows.

### Modified Capabilities

- `gateway-session-auth`: No requirement changes; implementation detail only.

## Impact

- **`src/collector/gateway_client.rs`**: `MeterObject` gains `act_energy_rcvd`; `MeterReadings` gains `grid_export_cum_wh`.
- **`src/collector/window_aggregator.rs`**: `CumulativeReading` fields change; `compute_delta` logic changes.
- **`src/collector/scheduler.rs`**: Populates new `CumulativeReading` fields; persists new `config_store` key.
- **`tests/unit/window_aggregator_test.rs`**: Existing tests updated; new regression test added.
- **`tests/integration/gateway_client_test.rs`**: Mock JSON updated to include `actEnergyRcvd`.
- **No API contract changes** — `EnergyWindow` response shape is unchanged; only the values become correct.
- **No DB schema changes** — `energy_window` table columns are unchanged.
