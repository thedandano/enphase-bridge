## 1. Gateway Client — deserialize actEnergyRcvd

- [x] 1.1 Add `act_energy_rcvd: f64` field to `MeterObject` in `gateway_client.rs` with `#[serde(rename = "actEnergyRcvd", default)]`
- [x] 1.2 Add `grid_import_cum_wh: f64` and `grid_export_cum_wh: f64` fields to `MeterReadings`
- [x] 1.3 In `request_meter_readings`, populate `grid_import_cum_wh` from `net.act_energy_dlvd` and `grid_export_cum_wh` from `net.act_energy_rcvd` (EID `1023410688`); log a warning if EID_NET is absent

## 2. Window Aggregator — correct energy balance

- [x] 2.1 Replace `consumption_wh: f64` in `CumulativeReading` with `grid_import_cum_wh: f64` and `grid_export_cum_wh: f64`
- [x] 2.2 Rewrite `compute_delta` to compute:
  - `wh_grid_import = delta(grid_import_cum_wh).max(0)`
  - `wh_grid_export = delta(grid_export_cum_wh).max(0)`
  - `wh_consumed = (wh_produced + wh_grid_import - wh_grid_export).max(0)`
  - Remove the old derived `wh_grid_import`/`wh_grid_export` lines

## 3. Scheduler — populate and persist new fields

- [x] 3.1 Update `CumulativeReading` construction in `scheduler.rs` `run()` to use `readings.grid_import_cum_wh` and `readings.grid_export_cum_wh`
- [x] 3.2 Add `KEY_GRID_EXPORT_WH: &str = "last_cumulative_grid_export_wh"` constant; rename `KEY_CONS_WH` to `KEY_GRID_IMPORT_WH` (and update its string value to `"last_cumulative_grid_import_wh"`)
- [x] 3.3 Update `load_persisted_reading` to read the new keys and populate both `grid_import_cum_wh` and `grid_export_cum_wh`
- [x] 3.4 Update `persist_reading` to write both new keys

## 4. Unit Tests — window aggregator

- [x] 4.1 Update `test_compute_delta_net_export` to use the new `CumulativeReading` fields (`grid_import_cum_wh`, `grid_export_cum_wh`)
- [x] 4.2 Update `test_compute_delta_net_import` similarly
- [x] 4.3 Update `test_compute_delta_never_negative` similarly
- [x] 4.4 Add `test_compute_delta_stalled_import_during_export`: pass a `CumulativeReading` where `grid_import_cum_wh` delta is `0` and `grid_export_cum_wh` delta is `9.863`, assert `wh_consumed > 0`, `wh_grid_export ≈ 9.863`, `wh_grid_import == 0`

## 5. Integration Tests — gateway client

- [x] 5.1 Add a mock JSON response for `/ivp/meters/readings` that includes `actEnergyRcvd` fields on all three meter objects
- [x] 5.2 Write `test_get_meter_readings_parses_grid_export`: verify `MeterReadings.grid_export_cum_wh` is populated from `actEnergyRcvd` on EID_NET
- [x] 5.3 Write `test_get_meter_readings_missing_eid_net_defaults_zero`: verify `grid_import_cum_wh` and `grid_export_cum_wh` are `0.0` when EID_NET is absent

## 6. Documentation

- [x] 6.1 Update `CLAUDE.md` key design decision (currently line ~66): replace "Grid import/export is derived from the energy balance (`produced - consumed`)" with the corrected description — grid import/export are read from the net meter's `actEnergyDlvd`/`actEnergyRcvd` counters; consumption is derived from the energy balance (`produced + grid_import - grid_export`)
- [x] 6.2 `README.md` — no changes needed (feature bullet is still accurate)

## 7. Verify

- [x] 7.1 Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` with no warnings
- [x] 7.2 Run `cargo test` — all tests pass
