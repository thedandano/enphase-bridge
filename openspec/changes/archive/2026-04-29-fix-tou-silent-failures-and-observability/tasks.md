## 0. Shared Utilities (do this first — later tasks add new call sites)

- [x] 0.1 Create `src/util.rs` (or `src/time.rs`) with a `pub fn unix_now() -> i64` function
- [x] 0.2 Remove the duplicated `unix_now()` helper from `src/api/handlers/trueup.rs`, `src/api/handlers/tou.rs`, and `src/api/handlers/health.rs`; replace each with `crate::util::unix_now()`
- [x] 0.3 Expose the module in `src/lib.rs` and `src/main.rs` as needed

## 1. Error Handling — Calculator Hard-Fails on Bad Rate Data

> **Complete group 1 before group 2.** Tracing in group 2 is placed after the parse calls; if group 2 runs first the trace lines would precede the new early-return paths.

- [x] 1.1 In `src/trueup/calculator.rs::parse_period_rates`, replace `tier["rate"].as_f64().unwrap_or(0.0)` with `ok_or_else(|| TouError::ParseError(format!("missing 'rate' field in tier {i}")))` and propagate with `?`
- [x] 1.2 In `src/trueup/calculator.rs::parse_period_rates`, replace `tier["sell"].as_f64().unwrap_or(rate)` with an explicit block: emit `tracing::warn!(event="tou_sell_rate_missing", tier=i)` and default to `rate` (not a hard error — sell-rate absence is acceptable per spec)
- [x] 1.3 In `src/trueup/calculator.rs` line ~79, replace `sched.get(month).and_then(|m| m.get(hour)).copied().unwrap_or(0)` with `ok_or_else(|| TouError::ParseError(format!("schedule out of bounds: month={month}, hour={hour}")))` and propagate with `?`

## 2. Observability — Tracing in Calculator and OpenEI Client

- [x] 2.1 Add `tracing::info!(event="trueup_calc_start", windows=%windows.len(), schedule_id=%schedule.id, period_count=%period_rates.len())` at the start of `calculator::calculate`, after `period_rates` is parsed (relies on group 1 being done)
- [x] 2.2 Add `tracing::info!(event="trueup_calc_done", peak_kwh, offpeak_kwh, super_offpeak_kwh, net_cost_usd)` before the `Ok(CalculatorResult {...})` return
- [x] 2.3 Add `tracing::warn!(event="tou_period_count_unexpected", n=%n)` in `build_period_map` when `n < 2 || n > 4`
- [x] 2.4 Add `tracing::debug!(event="tou_period_map")` logging the period→TouPeriod mapping after `build_period_map` returns
- [x] 2.5 In `src/tou/openei_client.rs`, before returning the "rate not found" error, collect available item names from the response and emit `tracing::error!(event="tou_rate_not_found", rate_label=%self.rate_label, available=?available_names)`

## 3. End-Bound Fix — Inclusive End Date for Estimate

- [x] 3.1 In `src/api/handlers/trueup.rs::get_estimate`, move the existing `period_end <= period_start` validation guard to execute **before** the +1-day normalization (not after)
- [x] 3.2 After the guard, add `let period_end = period_end + 86_400;` so the end date is inclusive
- [x] 3.3 Add a doc comment above the normalization line: "energy_window::query_range uses exclusive end (`window_start < ?`); add one day so the user-supplied UTC midnight date is inclusive. Callers must pass `end` as a UTC instant."

## 4. Error Mapping — TouError::ParseError → HTTP 502

- [x] 4.1 In `src/error.rs::IntoResponse for AppError`, add an explicit match arm for `AppError::Tou(TouError::ParseError(m))` returning `(StatusCode::BAD_GATEWAY, "upstream_parse_error", m.clone())`

## 5. Estimate Persistence — Wire Up true_up::insert (best-effort)

- [x] 5.1 In `src/api/handlers/trueup.rs::get_estimate`, after `calculator::calculate` returns `Ok(result)`, construct a `TrueUpEstimate` from the result and call `crate::storage::true_up::insert(&state.pool, &estimate).await`
- [x] 5.2 Wrap the insert call: if it returns `Err(e)`, log `tracing::error!(event="trueup_persist_failed", error=%e)` and continue to build the HTTP 200 response (best-effort — do NOT propagate with `?`)
- [x] 5.3 Remove `#[allow(dead_code)]` from `src/storage/true_up.rs::insert` and from `TrueUpEstimate` in `src/storage/models.rs` if present

## 6. TOU Schedule Lifecycle — Startup Probe

- [x] 6.1 Create `src/tou/probe.rs` with `pub async fn probe_tou_schedule(pool: &SqlitePool, rate_label: &str)` that calls `tou_schedule::query_latest` and emits `tracing::info!(event="tou_schedule_ok", age_days=<n>)` or `tracing::warn!(event="tou_schedule_stale", age_days=<n or null>)` based on presence and age vs the 90-day alarm threshold; returns `()` always
- [x] 6.2 Add `pub mod probe;` to `src/tou/mod.rs` so `main.rs` can reference it
- [x] 6.3 Call `tou::probe::probe_tou_schedule(&pool, &config.tou.rate_label).await` in `src/main.rs` after `check_jwt()` validation, before the `tokio::select!` block
- [x] 6.4 Confirm the probe never calls `process::exit` or returns `Err`; it is warn-only

## 7. TOU Schedule Lifecycle — Background Auto-Refresh Task

- [x] 7.1 Create `src/tou/refresh.rs` with `pub async fn run_tou_refresh_loop(pool: SqlitePool, api_key: String, utility_eia_id: u32, rate_label: String)`. The function body:
  - At entry: if `tou_schedule::query_latest` returns `None` or a row older than 7 days, call `do_refresh(...)` immediately (before the first sleep). Note: this 7-day refresh trigger is intentionally tighter than the 90-day health alarm.
  - Then: `loop { tokio::time::sleep(Duration::from_secs(7 * 24 * 3600)).await; if let Err(e) = do_refresh(...).await { tracing::error!(event="tou_refresh_error", error=%e); } }`
  - `do_refresh(pool, api_key, utility_eia_id, rate_label)` is a private async helper that constructs `OpenEiClient::new(api_key, utility_eia_id, rate_label)`, calls `.fetch().await`, then calls `tou_schedule::insert`; returns `Result<(), AppError>`
  - The outer loop body MUST NOT use `?`; all errors are consumed by the `if let Err` branch
- [x] 7.2 Add `pub mod refresh;` to `src/tou/mod.rs` so `main.rs` can reference it
- [x] 7.3 Add `tou::refresh::run_tou_refresh_loop(pool.clone(), config.tou.openei_api_key.clone(), config.tou.utility_eia_id, config.tou.rate_label.clone())` as a branch in the `tokio::select!` in `src/main.rs`
- [x] 7.4 Add a unit test for `do_refresh` error resilience: use mockito to make the OpenEI stub return a non-200 response, call `run_tou_refresh_loop` with a very short sleep interval, drive it through one failure cycle, and assert that the loop does not panic and the existing `tou_rate_schedule` row in the DB is unchanged

## 8. Health Endpoint — TOU Readiness Fields

- [x] 8.1 In `src/api/handlers/health.rs`, call `tou_schedule::query_latest(&state.pool, &state.tou_rate_label).await` (verify `state.tou_rate_label` is accessible via `AppState`; add it if missing)
- [x] 8.2 Add `tou_schedule_id: Option<i64>`, `tou_fetched_at: Option<i64>`, and `tou_stale: bool` to the health response struct and JSON output
- [x] 8.3 Set `tou_stale = true` when no schedule row exists OR when `fetched_at` is more than 90 days before the current `unix_now()` (use `crate::util::unix_now()` from group 0)

## 9. Tests — Calculator Edge Cases

- [x] 9.1 Add a test in `tests/unit/calculator_test.rs` for a 2-period schedule: assert `super_off_peak.import_kwh == 0.0` and that OffPeak captures all non-peak windows
- [x] 9.2 Add a test for a 4-period schedule: assert all four periods are represented and the middle two rank as OffPeak
- [x] 9.3 Add a test for tied rates (two periods with identical rate values): call `calculate` twice and assert identical output both times (deterministic behavior)
- [x] 9.4 Add a test for a tier missing the `"rate"` key: assert `calculate` returns `Err(AppError::Tou(TouError::ParseError(_)))`
- [x] 9.5 Add a test for a schedule array with fewer than 12 months: assert `calculate` returns `Err(AppError::Tou(TouError::ParseError(_)))`
- [x] 9.6 Fix the wrong assertion in `tests/integration/trueup_test.rs`: change `0.05` to `0.045` and tighten tolerance from `< 0.01` to `< 1e-6`
- [x] 9.7 Add a test for a missing `"sell"` key: assert `calculate` returns `Ok` (not `Err`) and uses the buy rate as the sell rate

## 10. Tests — Real TOU-DR-2 Fixture

- [x] 10.1 Create `tests/fixtures/sdge_tou_dr2_item.json` with a structurally accurate SDG&E TOU-DR-2 OpenEI item payload (3+ periods, realistic keys including `energyratestructure`, `energyweekdayschedule`, `energyweekendschedule`)
- [x] 10.2 Add a unit test in `tests/unit/calculator_test.rs` that deserializes the fixture into a `TouRateSchedule` and calls `calculate` with a known weekday peak-hour UTC timestamp, asserting `peak.import_kwh > 0.0`

## 11. Tests — OpenEI Client (mockito)

- [x] 11.1 Add `tests/unit/openei_client_test.rs` with mockito stubs for the OpenEI endpoint
- [x] 11.2 Test: successful fetch returns correct `rate_label`, `utility_name`, and non-empty `rate_json`
- [x] 11.3 Test: label not found → `Err(AppError::Tou(TouError::ParseError(_)))`
- [x] 11.4 Test: multiple items with same name, different `startdate` → most recent is selected
- [x] 11.5 Test: non-200 HTTP response → `Err(AppError::Tou(TouError::UpstreamUnavailable(_)))`

## 12. Tests — POST /api/tou/refresh Integration

- [x] 12.1 Add a test in `tests/integration/` that stubs OpenEI via mockito, posts to `/api/tou/refresh`, and asserts HTTP 200 with a valid `schedule_id` in the response body
- [x] 12.2 Assert that a `tou_rate_schedule` row was inserted with the correct `rate_label` and non-empty `rate_json`

## 13. Tests — End-Bound Regression

- [x] 13.1 Add an integration test in `tests/integration/trueup_test.rs` that seeds a window with `window_start` at UTC midnight of the `end` date and asserts it appears in the estimate result (confirming the +1-day fix)
- [x] 13.2 Add a test asserting that `start == end` (same-day) is accepted and returns HTTP 200 covering one day of data

## 14. Documentation Updates

- [x] 14.1 Update `CLAUDE.md` key design decisions: document the exclusive-end SQL contract (`window_start < ?`) and the `get_estimate` +1-day UTC normalization; note that `GET /api/energy/windows` retains exclusive-end semantics
- [x] 14.2 Update `CLAUDE.md` key design decisions: document the rate-ranking algorithm invariant (`n ≥ 3` required for SuperOffPeak; ties result in non-deterministic Super Off-Peak assignment without secondary sort), and the `sell`-rate warning fallback
- [x] 14.3 Update `CLAUDE.md` module map: add `src/tou/probe.rs` and `src/tou/refresh.rs` with one-line descriptions; update scheduler/startup description to reflect `main.rs` calls the probe
- [x] 14.4 Update `CLAUDE.md` testing approach: note that `tests/fixtures/sdge_tou_dr2_item.json` exists and is used to catch OpenEI schema drift
- [x] 14.5 Move the duplicated `fixture_rate_json()` helper and the three shared timestamp constants (`PEAK_TS`, `SUPER_OP_TS`, `OFF_PEAK_TS`) into `tests/common/mod.rs`; add `mod common;` to `tests/unit.rs` and `tests/integration.rs`; import via `use super::common::...` in each test file

## 15. Final Validation

- [x] 15.1 Run `cargo fmt --check` and fix any formatting issues
- [x] 15.2 Run `cargo clippy --all-targets -- -D warnings` and resolve all warnings
- [x] 15.3 Run `cargo test` and confirm all tests pass (unit + integration)
- [x] 15.4 Add inline `#[cfg(test)]` tests to `src/tou/probe.rs` using the `tracing-test` crate (dev-dependency) and `#[traced_test]` macro to assert the correct log event (`tou_schedule_ok` or `tou_schedule_stale`) is emitted for each case: no schedule, fresh schedule, and over-90-day stale schedule. The wiring in `main.rs` is a one-liner verified by code review.
