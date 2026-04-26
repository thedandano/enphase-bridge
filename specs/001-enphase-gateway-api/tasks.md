---
description: "Task list for Enphase Gateway Data Service"
---

# Tasks: Enphase Gateway Data Service

**Input**: Design documents from `/specs/001-enphase-gateway-api/`
**Prerequisites**: plan.md ✅, spec.md ✅, data-model.md ✅, contracts/api.md ✅, research.md ✅

**TDD**: Constitution Principle II mandates test-first. Test tasks are REQUIRED and MUST fail before implementation begins (Red → Green → Refactor).

**Organization**: Tasks are grouped by vertical slice / user story to enable independent implementation and testing of each deliverable.

## Format: `[ID] [P?] [Story?] Description — file path`

- **[P]**: Can run in parallel (different files, no shared incomplete dependency)
- **[Story]**: Which user story this task belongs to (US1–US4)

---

## Phase 1: Setup

**Purpose**: Initialize the Rust project and shared tooling. No business logic.

- [X] T001 Initialize Rust binary project — `cargo new --bin enphase-ds` at repo root
- [X] T002 Add all dependencies to `Cargo.toml` — tokio (full), axum, reqwest (rustls-tls), sqlx (sqlite + runtime-tokio), jsonwebtoken, figment (toml + env), tracing, tracing-subscriber (json), serde, serde\_json
- [X] T003 [P] Create source directory structure — `src/auth/`, `src/collector/`, `src/inverter/`, `src/storage/`, `src/api/handlers/`, `src/tou/`, `src/trueup/`, `tests/integration/`, `tests/unit/`
- [X] T004 [P] Create example config file — `config.example.toml` with all sections (gateway, polling, api, storage, tou) matching `src/config.rs` types
- [X] T005 [P] Create `.gitignore` — exclude `config.toml`, `energy.db`, `target/`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure shared by all user stories. MUST complete before any story phase begins.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T006 Define config types in `src/config.rs` — figment-based TOML + env merge; structs: `GatewayConfig` (host, token), `PollingConfig` (interval\_secs), `ApiConfig` (host, port), `StorageConfig` (db\_path), `TouConfig` (openei\_api\_key, sdge\_rate\_label)
- [X] T007 Define error types in `src/error.rs` — `AppError` enum: `GatewayError`, `AuthError`, `StorageError`, `TouError`, `ApiError`; implement `std::error::Error` and `axum::response::IntoResponse`
- [X] T008 [P] Create SQLite migrations in `migrations/` — four `CREATE TABLE` statements: `energy_window`, `microinverter_snapshot`, `tou_rate_schedule`, `true_up_estimate`, `config_store` with indexes per data-model.md
- [X] T009 [P] Implement SQLite connection pool in `src/storage/db.rs` — WAL mode, `max_connections(1)`, `create_if_missing(true)`, `foreign_keys(true)`, run migrations on startup
- [X] T010 [P] Define Rust storage model types in `src/storage/models.rs` — `EnergyWindow`, `MicroinverterSnapshot`, `TouRateSchedule`, `TrueUpEstimate` structs with `sqlx::FromRow` + `serde::Serialize/Deserialize` derives per data-model.md
- [X] T011 [P] Configure structured logging in `src/main.rs` — `tracing_subscriber` with JSON formatter; log level from env (`RUST_LOG`); redact token fields

**Checkpoint**: `cargo build` succeeds; `cargo test` passes (no logic yet, just compilation).

---

## Phase 3: US2 — Authenticate with Local Gateway (Priority: P1) 🎯 MVP Entry

**Goal**: Service starts, authenticates with the IQ Gateway using a configured JWT, and logs a successful data response. No storage yet.

**Independent Test**: Start binary → see structured log line with `wNow` value from `/ivp/meters/readings` → `cargo test --test auth_test` passes.

### Tests for US2 — write first, verify they FAIL before implementing

- [X] T012 [US2] Write unit test for JWT expiry detection in `tests/unit/token_manager_test.rs` — test cases: valid token (exp = now + 1yr), near-expiry (exp = now + 5min), expired (exp = now - 1min); assert correct `is_near_expiry()` result

### Implementation for US2

- [X] T013 [US2] Implement `src/auth/token_manager.rs` — load JWT string from `GatewayConfig.token`, decode header+claims via `jsonwebtoken` (no signature verification for local use), expose `is_near_expiry(threshold: Duration) -> bool`, `expiry_timestamp() -> i64`
- [X] T014 [US2] Implement `src/collector/gateway_client.rs` — `reqwest::Client` with `danger_accept_invalid_certs(true)`, base URL from config, `get_meter_readings() -> Result<MeterReadings, AppError>` (GET `/ivp/meters/readings`, `Authorization: Bearer <token>` header, deserialize response)
- [X] T015 [US2] Write integration test in `tests/integration/auth_test.rs` — mock or live gateway: assert `gateway_client.get_meter_readings()` returns `Ok` with positive `wNow` field; assert structured log contains `"event":"gateway_poll_success"`
- [X] T016 [US2] Wire auth startup into `src/main.rs` — load config → build token\_manager → build gateway\_client → call `get_meter_readings()` → log success with `wNow` value, or `AppError::AuthError` halts process with clear log

**Checkpoint**: US2 independently testable — `cargo run` logs gateway wNow reading then exits (no daemon loop yet).

---

## Phase 4: US1 — Poll and Store Solar Data (Priority: P1)

**Goal**: Headless data logger. Polls every 60s, closes 15-minute windows, stores `energy_window` and `microinverter_snapshot` rows to SQLite.

**Independent Test**: Run for 15+ minutes → `SELECT * FROM energy_window` returns at least one row; `SELECT COUNT(*) FROM microinverter_snapshot` = N inverters; `cargo test --test collector_test` passes.

### Tests for US1 — write first, verify they FAIL before implementing

- [ ] T017 [US1] Write unit test for window boundary + delta computation in `tests/unit/window_aggregator_test.rs` — test: given two cumulative readings 15 min apart, assert `compute_delta()` returns correct `wh_produced`, `wh_consumed`, `wh_grid_import`, `wh_grid_export`; test `is_window_boundary(ts)` for exact and non-boundary timestamps
- [ ] T018 [P] [US1] Write unit test for devstatus parser in `tests/unit/snapshot_test.rs` — given fixture JSON from `/ivp/peb/devstatus`, assert parser returns correct `Vec<MicroinverterSnapshot>` with `watts_output` (mW ÷ 1000) and `is_online` per serial

### Implementation for US1

- [ ] T019 [US1] Implement `src/collector/window_aggregator.rs` — `compute_window_boundary(ts: i64) -> i64` (floor to 15-min epoch), `compute_delta(prev: CumulativeReading, curr: CumulativeReading) -> EnergyWindow`, track `is_complete` flag (set false if any poll errored during window)
- [ ] T020 [US1] Implement `src/collector/scheduler.rs` — tokio interval loop at `poll_interval_secs`; on each tick: call `gateway_client.get_meter_readings()`, pass to `window_aggregator`; at 15-min boundary: finalize window → write to storage → trigger inverter snapshot
- [ ] T021 [US1] Implement `src/storage/energy_window.rs` — `insert(pool, window: EnergyWindow) -> Result<()>`, `query_range(pool, start: i64, end: i64, limit: i32, offset: i32) -> Result<Vec<EnergyWindow>>`, `query_latest(pool) -> Result<Option<EnergyWindow>>`
- [ ] T022 [P] [US1] Implement `src/inverter/snapshot.rs` — load serial numbers from `GET /inventory.json` at startup (cache in `config_store`); at window boundary: `GET /ivp/peb/devstatus`, parse `pcu.fields` + `pcu.values` matrix, find `acPowerINmW` column, divide by 1000 for watts; mark absent inverters `is_online = false`
- [ ] T023 [P] [US1] Implement `src/storage/inverter_snapshot.rs` — `insert_batch(pool, snapshots: Vec<MicroinverterSnapshot>) -> Result<()>`, `query_by_window(pool, window_start: i64) -> Result<Vec<MicroinverterSnapshot>>`, `query_by_serial_range(pool, serial: &str, start: i64, end: i64, limit: i32, offset: i32) -> Result<Vec<MicroinverterSnapshot>>`
- [ ] T024 [US1] Implement `src/storage/config_store.rs` — `get(pool, key: &str) -> Result<Option<String>>`, `set(pool, key: &str, value: &str) -> Result<()>`; persist `last_window_start` and `last_cumulative_produced/consumed/import/export` across restarts
- [ ] T025 [US1] Write integration test in `tests/integration/collector_test.rs` — inject mock gateway responses for two poll cycles spanning a window boundary; assert one `energy_window` row inserted with correct delta values; assert `microinverter_snapshot` rows inserted with correct watt values

**Checkpoint**: `cargo run` runs as daemon for 15+ min; direct SQLite query shows `energy_window` and `microinverter_snapshot` rows accumulating.

---

## Phase 5: US3 — Query Per-Inverter and Aggregate Energy Data via API (Priority: P2)

**Goal**: HTTP read API serving energy windows and inverter snapshots. No auth on API (LAN-trust).

**Independent Test**: `curl http://localhost:8080/api/energy/windows/latest` returns JSON with energy fields; `curl http://localhost:8080/api/inverters/snapshots/window/{ts}` returns per-inverter array; `cargo test --test api_energy_test api_inverter_test` passes.

### Tests for US3 — write first, verify they FAIL before implementing

- [ ] T026 [US3] Write contract test for energy endpoints in `tests/integration/api_energy_test.rs` — test `GET /api/energy/windows` with valid/invalid ranges; test `GET /api/energy/windows/latest`; assert response schema matches contracts/api.md; assert 400 on invalid range; assert 404 when no data
- [ ] T027 [P] [US3] Write contract test for inverter endpoints in `tests/integration/api_inverter_test.rs` — test `GET /api/inverters/snapshots` with serial filter; test `GET /api/inverters/snapshots/window/{ts}`; assert correct schema; assert 404 on unknown window

### Implementation for US3

- [ ] T028 [US3] Implement axum server in `src/api/server.rs` — `AppState` struct holding `SqlitePool`; router with all routes wired; `bind(host:port)`; spawn as concurrent tokio task from `main.rs`
- [ ] T029 [US3] Implement `src/api/handlers/energy.rs` — `get_windows(State, Query)` handler: parse `start`, `end`, `limit`, `offset`; call `energy_window::query_range()`; return paginated JSON; `get_latest(State)` handler: call `energy_window::query_latest()`; 404 if none
- [ ] T030 [P] [US3] Implement `src/api/handlers/inverters.rs` — `get_snapshots(State, Query)` handler: optional `serial` filter, time range, pagination; `get_snapshots_by_window(State, Path)` handler: single window timestamp → all inverters
- [ ] T031 [P] [US3] Implement `src/api/handlers/health.rs` — `GET /api/health`: read `last_window_start` from `config_store`, read `token_expires_at` from token\_manager, compute uptime from process start; return JSON per contracts/api.md
- [ ] T032 [US3] Wire API server startup into `src/main.rs` — `tokio::spawn` axum server task alongside polling scheduler; both run concurrently on same runtime

**Checkpoint**: US3 independently testable — `curl http://localhost:8080/api/energy/windows/latest` and inverter endpoints return correct JSON.

---

## Phase 6: US4 — Estimate NEM True-Up Cost Using SDGE TOU Rates (Priority: P2)

**Goal**: Fetch SDGE TOU schedule from OpenEI URDB; compute NEM true-up estimate per TOU period; expose via API.

**Independent Test**: `curl -X POST http://localhost:8080/api/tou/refresh` stores schedule; `curl "http://localhost:8080/api/trueup/estimate?start=...&end=..."` returns breakdown with net\_cost\_usd; `cargo test --test trueup_test` passes with fixture data.

### Tests for US4 — write first, verify they FAIL before implementing

- [ ] T033 [US4] Write unit test for true-up calculator in `tests/unit/calculator_test.rs` — fixture: 10 EnergyWindows across peak/off-peak/super-off-peak hours + known SDG&E rates; assert `calculate()` returns exact import/export kWh per period and correct `net_cost_usd`
- [ ] T034 [P] [US4] Write integration test for TOU + estimate endpoint in `tests/integration/trueup_test.rs` — seed fixture `tou_rate_schedule` and `energy_window` rows; call `GET /api/trueup/estimate`; assert response schema and net\_cost\_usd matches calculator unit test expectation

### Implementation for US4

- [ ] T035 [US4] Implement `src/tou/openei_client.rs` — separate `reqwest::Client` (normal TLS); `fetch_sdge_rates(api_key, rate_label) -> Result<String>` calls `https://api.openei.org/utility_rates?version=7&format=json&ratesforutility=San+Diego+Gas+%26+Electric&sector=Residential&detail=full`; returns raw JSON string for storage
- [ ] T036 [US4] Implement `src/storage/tou_schedule.rs` — `insert(pool, utility_name, rate_label, effective_date, rate_json) -> Result<i64>`; `query_for_date(pool, rate_label, date: i64) -> Result<Option<TouRateSchedule>>` (latest version with `effective_date` ≤ given date)
- [ ] T037 [US4] Implement `src/trueup/calculator.rs` — parse `rate_json` to extract `energyweekdayschedule`, `energyweekendschedule`, `energyratestructure`; iterate `EnergyWindow` rows in period; classify each window by TOU period (peak/off-peak/super-off-peak) using hour + day-of-week; accumulate import/export kWh and cost per period; return `TrueUpEstimate`
- [ ] T038 [US4] Implement `src/storage/true_up.rs` — `insert(pool, estimate: TrueUpEstimate) -> Result<i64>`, `query_latest_for_period(pool, start: i64, end: i64) -> Result<Option<TrueUpEstimate>>`
- [ ] T039 [US4] Implement `src/api/handlers/tou.rs` — `POST /api/tou/refresh`: call `openei_client.fetch_sdge_rates()`, store via `tou_schedule::insert()`, return schedule\_id + metadata; log success with schedule version
- [ ] T040 [US4] Implement `src/api/handlers/trueup.rs` — `GET /api/trueup/estimate`: validate `start`/`end` params; load `tou_rate_schedule` for period; load `energy_window` rows; call `calculator::calculate()`; store result; return breakdown JSON per contracts/api.md; 422 if no TOU schedule or no data

**Checkpoint**: All four user stories independently functional. Full system demonstrable end-to-end.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Quality, security, and operational hardening across all stories.

- [ ] T041 Add data retention pruning in `src/storage/db.rs` — archive `energy_window` and `microinverter_snapshot` rows older than 13 months to `energy_archive.db` on startup and weekly via tokio scheduled task
- [ ] T042 [P] Add `--check-auth` CLI flag in `src/main.rs` — one-shot mode: validate token, call gateway, print result, exit; useful for setup verification
- [ ] T043 [P] Audit all `tracing` log statements — grep for any `token`, `bearer`, `jwt`, `api_key` in log output; ensure redaction; add test asserting log output for known-sensitive operations contains `[REDACTED]`
- [ ] T044 [P] Add CI configuration — `Makefile` or `.github/workflows/ci.yml` with targets: `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test`, optional `cargo audit`
- [ ] T045 Run `quickstart.md` validation — build release binary, execute all steps in `quickstart.md`, verify each endpoint returns expected response, confirm no regressions

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories
- **US2 (Phase 3)**: Depends on Foundational — entry point for all story work
- **US1 (Phase 4)**: Depends on US2 (auth must work before polling begins)
- **US3 (Phase 5)**: Depends on US1 (data must exist to query)
- **US4 (Phase 6)**: Depends on US3 (API server must be running)
- **Polish (Phase 7)**: Depends on all stories complete

### Within Each Story

- Tests MUST be written first and MUST FAIL before implementation
- Models/types before services
- Services before handlers
- Story complete + checkpointed before next story begins

### Parallel Opportunities (within phase)

**Foundational (Phase 2)**:
```
T008 (db.rs pool) ──┐
T009 (migrations)   ├── all parallel after T006, T007
T010 (models.rs)    │
T011 (logging)   ───┘
```

**US1 (Phase 4)**:
```
T017 (window unit test)      ──┐
T018 (snapshot unit test)    ──┤  parallel tests
T019 (window_aggregator)     ──┤
T020 (scheduler)             ──┤
T021 (energy_window storage) ──┤  parallel implementation
T022 (inverter/snapshot)     ──┤
T023 (inverter storage)      ──┘
```

**US3 (Phase 5)**:
```
T026 (energy contract test)   ──┐
T027 (inverter contract test) ──┤  parallel tests
T029 (energy handler)         ──┤
T030 (inverter handler)       ──┤  parallel handlers
T031 (health handler)         ──┘
```

---

## Implementation Strategy

### MVP First (US2 + US1 only — Slices 1 & 2)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: US2 (auth + connectivity) — **VALIDATE independently**
4. Complete Phase 4: US1 (polling + storage) — **VALIDATE: SQLite has rows**
5. **STOP and DEMO**: Headless data logger running on home server

### Incremental Delivery

1. Foundation → US2 → **Checkpoint** (gateway proven)
2. → US1 → **Checkpoint** (data flowing into SQLite)
3. → US3 → **Checkpoint** (API queryable by curl or app)
4. → US4 → **Checkpoint** (true-up estimable for any date range)
5. Each checkpoint = independently deployable and demonstrable

### Parallel Subagent Strategy

With two subagents after US2 passes:
- **Subagent A**: US1 window aggregation (T017–T021, T024–T025)
- **Subagent B**: US1 inverter snapshots (T018, T022–T023)
- Both merge before US3 begins

---

## Notes

- `[P]` tasks touch different files — no merge conflicts when run in parallel
- Tests marked `[US?]` map directly to spec.md user stories for traceability
- Every story checkpoint validates the story independently before proceeding
- Token values MUST NOT appear in any log output (verified in T043)
- `config.toml` is never committed — verify `.gitignore` in T005 before any commit
