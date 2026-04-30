## Why

The TOU / true-up feature silently produces wrong results: the dashboard shows Peak = 0.00 kWh over 30 California spring days, numbers never update, and there is no log evidence to help diagnose why. A council review identified three independent root causes — a silent zero-rate fallback in the calculator, an exclusive end-bound in the energy window query that drops the current day, and no automatic TOU schedule lifecycle — along with pervasive gaps in observability, persistence, and test coverage.

## What Changes

- **Calculator error handling**: replace `unwrap_or(0.0)` on rate fields and `unwrap_or(0)` on schedule index lookups with explicit `TouError::ParseError` propagation.
- **Observability**: add `tracing::info!` / `tracing::warn!` calls at every silent-failure site in `calculator.rs`, `openei_client.rs`, and the estimate handler.
- **End-bound bug**: clarify and document the exclusive `window_start < ?` contract; update the estimate handler to use an inclusive end or add a sentinel day so the current day is never dropped.
- **TOU schedule lifecycle**: add a startup probe that validates a schedule exists (warn and continue, same pattern as `probe_meters`) and a weekly auto-refresh background task in `tokio::select!`.
- **Estimate persistence**: wire up `true_up::insert` in the estimate handler; remove `#[allow(dead_code)]`.
- **Health endpoint**: add `tou_schedule_id`, `tou_fetched_at`, and `tou_stale` fields.
- **Error mapping**: map `TouError::ParseError` to 502 in `IntoResponse` instead of falling through to 500.
- **Tests**: add coverage for real TOU-DR-2 fixture, `openei_client` unit tests (mockito), `POST /api/tou/refresh` integration test, 2-period and 4+-period schedule tests, tied-rate stability test, and fix the wrong assertion value in `trueup_test.rs`.
- **Documentation**: update `CLAUDE.md` (module map, key design decisions) and `specs/` to document the inclusive/exclusive end-bound contract and the rate-ranking invariant.

## Capabilities

### New Capabilities

- `tou-schedule-lifecycle`: Automatic TOU schedule validation at startup and periodic auto-refresh; staleness surfaced in health endpoint.
- `tou-observability`: Structured tracing for calculator execution, period classification, and schedule parsing; estimate results persisted to `true_up_estimate` table.

### Modified Capabilities

- `energy-metering`: The `query_range` end-bound contract changes (exclusive → documented inclusive-with-sentinel); affects the estimate handler and any caller that passes a bare date string.

## Impact

- **`src/trueup/calculator.rs`**: error propagation, tracing calls, period-count warning.
- **`src/tou/openei_client.rs`**: rate-not-found error logging.
- **`src/api/handlers/trueup.rs`**: end-bound fix, `true_up::insert` call, logging.
- **`src/api/handlers/tou.rs`**: TOU refresh; no functional change but gains integration test.
- **`src/api/handlers/health.rs`**: new TOU fields.
- **`src/collector/scheduler.rs`**: startup TOU probe (warn-only).
- **`src/main.rs`**: weekly TOU refresh task added to `tokio::select!`.
- **`src/storage/energy_window.rs`**: end-bound SQL contract documented.
- **`src/storage/true_up.rs`**: `#[allow(dead_code)]` removed; insert now called.
- **`src/error.rs`**: `TouError::ParseError` mapped to 502.
- **`tests/`**: new unit tests for calculator edge cases, mockito-backed `openei_client` tests, new integration tests for refresh and estimate.
- **`CLAUDE.md`**: updated module map, design decisions, and testing approach sections.
