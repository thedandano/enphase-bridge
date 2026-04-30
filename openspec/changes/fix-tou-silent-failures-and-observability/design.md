## Context

The TOU/true-up subsystem consists of three phases: (1) fetching a rate schedule from OpenEI and storing it in `tou_rate_schedule`; (2) querying energy windows from `energy_window` over a user-supplied date range; and (3) running `calculator::calculate` to classify each window into Peak/Off-Peak/Super Off-Peak and accumulate costs. All three phases have silent-failure modes that produce plausible-looking but wrong output. The daemon runs as a single process with two concurrent tasks (collector loop + Axum API server) joined via `tokio::select!` in `main.rs`.

## Goals / Non-Goals

**Goals:**

- All rate-parse failures propagate as explicit errors rather than silently defaulting to 0.0.
- The calculator emits structured trace events so an operator can verify classification from logs.
- The current-day exclusion bug is fixed and the end-bound contract is documented.
- A TOU schedule probe runs at startup and a weekly refresh runs in the background.
- True-up estimates are persisted to `true_up_estimate` after every successful computation.
- Health endpoint exposes TOU schedule presence and staleness.
- `TouError::ParseError` maps to HTTP 502, not 500.
- Test coverage added for: real TOU-DR-2 fixture, OpenEI client (mockito), `/api/tou/refresh` integration, 2-period / 4-period schedules, tied rates, and the corrected assertion value.
- `CLAUDE.md` and `specs/energy-metering/` updated to reflect the new contracts.

**Non-Goals:**

- UI changes to the dashboard (frontend is out of scope).
- Changing the rate-ranking algorithm to use OpenEI period names instead of rate-value ranking — the current heuristic is kept but now documented and warned on edge cases.
- Adding a history API endpoint for `true_up_estimate` rows (insert only for now).
- Multi-utility or multi-tariff support.

## Decisions

### D1: Calculator rate-parse must hard-fail, not default

**Decision:** Change `tier["rate"].as_f64().unwrap_or(0.0)` to `tier["rate"].as_f64().ok_or_else(|| TouError::ParseError("missing 'rate' field in tier N".into()))`. Same for the schedule-index `unwrap_or(0)` (change to `ok_or_else`). Propagate `Err(AppError::Tou(...))` out of `calculate()`.

The `sell` rate (NEM export credit) is treated differently: absence is not a hard error. If `tier["sell"]` is missing, emit `tracing::warn!(event="tou_sell_rate_missing", tier=N)` and fall back to the buy `rate`. This is acceptable because many residential tariffs do not publish a distinct sell rate and the common convention is to credit at the same rate.

**Rationale:** A zero rate is a valid value semantically (free energy tier); silently returning 0.0 for a parse failure is indistinguishable from a real $0 rate. Failing loudly is the only way to surface OpenEI schema drift. This is consistent with the project's "no silent failures" policy.

**Alternative considered:** Log a warning and continue with 0.0. Rejected — operators would need to monitor logs constantly to notice; the estimate would still silently be wrong.

### D2: End-bound fix — add one day to the exclusive end

**Decision:** In `trueup.rs::get_estimate`, after parsing `period_end` from the RFC-3339 string, add 86,400 seconds (one day) so that a date string like `2026-04-29T00:00:00Z` covers the full April 29. Document the half-open `[start, end)` contract in both the handler and `specs/energy-metering/spec.md`.

**UTC assumption:** The +86,400 normalization is UTC-correct. All timestamps in `energy_window.window_start` are Unix seconds (UTC). The +86,400 exactly covers one UTC calendar day. Users MUST pass `end` as a UTC midnight value (e.g., `2026-04-29T00:00:00Z`). Local-timezone interpretation is explicitly out of scope for this change (see Open Questions).

**Validation order:** The `end <= start` guard MUST execute against the raw parsed `period_end` value, before the +86,400 is applied. This ensures the API rejects semantically invalid ranges (same-day start/end is allowed and covers exactly one calendar day).

**Rationale:** The SQL uses `window_start < ?` (exclusive end). The UI sends midnight of the selected end date, so the entire last day is excluded. Adding one day at the handler layer is the minimal change: the SQL and `energy_window::query_range` contract stay unchanged (documented as exclusive), and the estimate handler normalizes the user-facing "inclusive date" into the half-open query range.

**Alternative considered:** Change the SQL to `<=`. Rejected — this would require updating all callers and tests, and a Unix timestamp for "end of day" is ambiguous (23:59:59 vs midnight next day). Normalizing at the handler is cleaner.

### D3: TOU schedule lifecycle — warn-only startup probe + weekly auto-refresh

**Decision:** At daemon startup (after `check_jwt()`), call `probe_tou_schedule(pool, rate_label)` from `src/tou/probe.rs`. This function queries `tou_schedule::query_latest`, logs `tou_schedule_ok` or `tou_schedule_stale`, and returns `()` — it never fails or exits the process.

The auto-refresh runs as a separate async function `run_tou_refresh_loop(pool, api_key, utility_eia_id, rate_label)` added to `tokio::select!` in `main.rs`. Both the probe and the refresh loop live in `src/tou/` to keep TOU lifecycle concerns colocated and separate from the Enphase gateway collector.

**Two distinct thresholds:** The TOU lifecycle uses two separate thresholds that must not be confused:
- **Refresh trigger (7 days):** The refresh loop checks whether the schedule is older than 7 days to decide whether to fire immediately at startup. This matches the 7-day sleep interval — any schedule older than one refresh cycle is re-fetched eagerly.
- **Health/probe stale alarm (90 days):** The startup probe and health endpoint flag the schedule as "stale" only when it is older than 90 days. This is an operator-visible alarm, not a refresh condition; it fires only when the 7-day auto-refresh has failed repeatedly for an extended period.

**Loop topology (critical for daemon stability):** The refresh loop body MUST NOT propagate errors with `?`. The correct structure is:

```
async fn run_tou_refresh_loop(...) {
    // Fire immediately if schedule is missing or older than 7 days
    if needs_refresh_now(&pool, &rate_label).await { let _ = do_refresh(...).await; }
    loop {
        tokio::time::sleep(Duration::from_secs(7 * 24 * 3600)).await;
        if let Err(e) = do_refresh(...).await {
            tracing::error!(event="tou_refresh_error", error=%e);
            // continue — do not return or propagate
        }
    }
}
```

`do_refresh` is a private async helper returning `Result<(), AppError>`. All errors are consumed inside `if let Err`. No `?` or `return` appears in the outer loop body.

**`tokio::select!` with three infinite-loop branches:** Adding the refresh loop to `tokio::select!` means the API server, collector, and refresh loop all share a single select. This matches the existing two-branch pattern. The tradeoff is that if any branch terminates (e.g., the API server returns `Err`), `select!` cancels the remaining branches. This is intentional: a fatal error in any core task shuts down the daemon cleanly. The refresh loop's strict `if let Err` discipline ensures it only terminates by explicit design, not by accident.

**Rationale:** Unlike the missing-meter probe (which halts because data collection is impossible without it), a stale TOU schedule degrades estimate accuracy but does not break the collector. A warn-and-continue approach is appropriate. A 7-day refresh cadence is sufficient for utility rates, which change monthly at most.

**Alternative considered:** Fail-fast on missing schedule. Rejected — the daemon is primarily a collector; the TOU feature is read-path only. Blocking startup on a missing external API key/schedule is too disruptive.

### D4: Estimate persistence — best-effort write to `true_up_estimate`

**Decision:** After `calculator::calculate` succeeds, call `true_up::insert` with the computed result. If `insert` returns `Err`, log `tracing::error!(event="trueup_persist_failed", error=%e)` and return HTTP 200 with the computed estimate anyway. Remove `#[allow(dead_code)]`.

**Persistence failure is best-effort:** Blocking the HTTP response on a write failure would degrade the dashboard for a non-critical audit feature. The estimate is correct regardless of whether it was persisted.

**Duplicate rows:** The `true_up_estimate` table has no unique constraint on `(period_start, period_end)`. Two concurrent requests for the same period will insert two rows. This is acceptable — the table is history-style audit storage, not a keyed cache. Duplicates are documented and intentional.

**Rationale:** Historical rows allow the operator (and future tooling) to answer "has this number changed?" — which was the exact symptom that triggered this investigation. The insert is cheap (one row per API call).

### D5: Tracing in calculator

**Decision:** Add one `tracing::info!(event="trueup_calc_start", windows, schedule_id, period_count)` before the loop, one `tracing::info!(event="trueup_calc_done", peak_kwh, offpeak_kwh, super_offpeak_kwh, net_cost_usd)` after, and `tracing::warn!(event="tou_period_count_unexpected", n)` when `n < 2` or `n > 4`. Add `tracing::debug!(event="tou_period_map", ?period_map)` for the resolved mapping.

**Rationale:** These log lines provide everything needed to diagnose a wrong result from a log file alone.

### D6: Test for real TOU-DR-2 structure

**Decision:** Add a `tests/fixtures/sdge_tou_dr2.json` file containing a minimal but structurally accurate TOU-DR-2 OpenEI item payload. Unit test: parse it through `calculator::calculate` and assert Peak > 0 kWh for a known weekday peak-hour timestamp. Integration test for `openei_client`: stub via mockito returning this fixture, assert `FetchedSchedule` fields.

**Rationale:** All current tests use synthetic 3-period fixtures. A real fixture catches key-name drift and structural assumptions.

## Risks / Trade-offs

- **D2 end-bound change may affect the energy windows endpoint** — `GET /api/energy/windows` uses `parse_iso_or` with the same exclusive-end semantics but does *not* add a day. We must NOT change the windows endpoint; only the trueup estimate handler gets the +1-day normalization. Risk of confusion: document clearly in both places.
- **D3 refresh loop must never escape with `?`** — see D3 loop topology above. Any `?` propagation or panic in the loop body would complete the `tokio::select!` branch and tear down the daemon. The loop body uses `if let Err(e) = ...` exclusively.
- **D1 hard-fail causes first-deploy 502 on existing rate_json** — if the stored `rate_json` row was fetched before this fix, it may now fail the stricter parse and return 502 until the operator runs `POST /api/tou/refresh`. See the Deployment section below.
- **`true_up_estimate` table has no upper bound** — estimates accumulate indefinitely. Not a concern for years of home use but worth noting.

## Deployment

After deploying this change:

1. Run `POST /api/tou/refresh` **before** testing `GET /api/trueup/estimate`. Existing `tou_rate_schedule` rows stored before this fix may have a `rate_json` structure that now fails the stricter rate-parse and returns HTTP 502. The refresh inserts a new row that passes the new parse.
2. Confirm in logs that `event="tou_schedule_ok"` or `event="tou_schedule_stale"` appears at startup (from `probe_tou_schedule`).
3. Confirm `event="trueup_calc_start"` and `event="trueup_calc_done"` appear when the dashboard fetches an estimate.

## Open Questions

- Should the weekly auto-refresh task replace the existing `tou_rate_schedule` row or always insert a new one? Current schema always inserts (never deletes); `query_latest` picks the newest. This is fine — no change needed.
- Should `GET /api/trueup/estimate` accept a `tz` parameter to interpret bare dates in the user's local timezone rather than UTC? Out of scope for this change but worth a follow-up.
