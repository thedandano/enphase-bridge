# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

`enphase-bridge` is a self-hosted Rust daemon that polls an Enphase IQ Gateway over HTTPS, aggregates solar energy readings into 15-minute windows, and serves them via a local REST API backed by SQLite. It runs 24/7 on a home server or Raspberry Pi.

## Commands

```bash
# Build
cargo build
cargo build --release

# Run (requires config.toml — see config.example.toml)
cargo run

# Lint and format
cargo fmt --check        # check formatting (CI enforces this)
cargo fmt                # auto-format
cargo clippy --all-targets -- -D warnings   # lint; --all-targets covers test code too

# Tests
cargo test               # all tests (unit + integration)
cargo test --test unit   # unit tests only
cargo test --test integration   # integration tests only
cargo test --test unit window_aggregator   # single test module by name
cargo test test_window_boundary  # single test function by name

# Log verbosity (structured JSON to stdout)
RUST_LOG=debug cargo run          # verbose; default filter is enphase_bridge=info
RUST_LOG=enphase_bridge=trace cargo run
```

CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` on every push/PR.

## Git Hooks

Pre-commit and pre-push hooks live in `.githooks/`. Activate per clone with: `git config core.hooksPath .githooks`
- **pre-commit**: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings`
- **pre-push**: `cargo test`

## Architecture

The daemon has two concurrent tasks (joined via `tokio::select!` in `main.rs`):

1. **Collector loop** (`src/collector/`) — polls the IQ Gateway on a configurable interval, computes 15-min energy deltas, and persists windows + inverter snapshots to SQLite.
2. **API server** (`src/api/`) — Axum HTTP server serving all REST routes from the SQLite database.

### Data flow

```
IQ Gateway (HTTPS + JWT + session cookie)
    → GatewayClient        (collector/gateway_client.rs)
    → WindowAggregator     (collector/window_aggregator.rs)  — floor-divides timestamps into 15-min buckets
    → Scheduler            (collector/scheduler.rs)          — detects window boundary crossings; writes windows + snapshots
    → SQLite (sqlx)        (storage/)
    → Axum handlers        (api/handlers/)                   — query-only reads
```

The scheduler persists the last cumulative reading (timestamp + Wh) to the `config_store` table so it survives restarts without re-polling.

### Key design decisions

- **Cumulative-to-delta conversion**: The gateway exposes lifetime `actEnergyDlvd`/`actEnergyRcvd` counters. `window_aggregator.rs` computes deltas between consecutive readings. Grid import (`grid_import_cum_wh`) and grid export (`grid_export_cum_wh`) are both sourced from the net-consumption meter (EID `704643584`, `measurementType: "net-consumption"`) — the only documented bidirectional top-level meter per the Enphase IQ Gateway Local APIs tech brief (Jan 2023). House consumption is derived from the energy balance (`produced + grid_import - grid_export`).
- **Startup meter probe**: At scheduler startup (after `check_jwt()`), `probe_meters()` calls `GET /ivp/meters` to discover available meters by `measurementType` and validates that a `net-consumption` meter with `state: "enabled"` is present. The scheduler halts with a `GatewayError::MissingMeter` if absent — no silent fallback to zero.
- **Single SQLite connection** (`max_connections(1)`) with WAL mode — avoids write contention while allowing concurrent reads.
- **Gateway session auth (firmware 7.x+)**: At scheduler startup, `check_jwt()` POSTs to `/auth/check_jwt` with the Bearer JWT to exchange it for a session cookie. `get_meter_readings()` also auto-retries with a fresh `check_jwt()` on any 401, so session expiry is handled transparently.
- **Gateway JWT is not verified** — `token_manager.rs` only decodes the `exp` claim to warn/fail on expiry. The gateway's ES256 signature is not validated.
- **Config layering**: `figment` merges `config.toml` → `ENPHASE__` environment variables. Section separator is `__` (e.g. `ENPHASE__API__PORT=9090`).
- **Optional Bearer auth** — implemented in `api/middleware/api_key.rs` and `startup.rs` but **not yet wired into `main.rs` or `config.rs`**. `api.require_auth` and `api.api_key` config fields are planned but not active. Keys use constant-time comparison (`subtle::ConstantTimeEq`); auto-generated keys are written to **stderr only** to avoid leaking into log aggregators.
- **TOU period classification**: `trueup/calculator.rs` ranks OpenEI rate periods by rate value — highest rate → Peak, lowest rate (when ≥3 periods exist) → Super Off-Peak, rest → Off-Peak. There are no explicit period name checks.
- **Consumption sign convention**: Enphase reports consumption `activePower` as a negative value. `gateway_client.rs` negates it to positive watts.
- **Inverter online threshold**: An inverter is considered offline if its last report timestamp is older than 20 minutes (`OFFLINE_THRESHOLD_SECS = 1200`).

### Module map

| Module | Responsibility |
|--------|---------------|
| `config.rs` | Figment-based config loading (TOML + env) |
| `collector/gateway_client.rs` | HTTPS client for IQ Gateway metering + inverter endpoints; manages session cookie |
| `collector/window_aggregator.rs` | 15-min window math: `window_boundary()`, `compute_delta()` |
| `collector/scheduler.rs` | Polling loop; detects window boundary crossings; persists via `config_store` |
| `inverter/snapshot.rs` | Parses gateway inverter reports into `MicroinverterSnapshot`; determines online status |
| `storage/db.rs` | SQLite connection pool setup + sqlx migrations |
| `storage/models.rs` | Shared data structs (`EnergyWindow`, `InverterSnapshot`, etc.) |
| `storage/{energy_window,inverter_snapshot,tou_schedule,true_up,config_store}.rs` | Per-table query functions |
| `api/server.rs` | Axum router + `AppState` definition |
| `api/handlers/` | One file per route group: `energy`, `inverters`, `arrays`, `tou`, `trueup`, `health` |
| `api/middleware/api_key.rs` | Bearer token middleware; key generation + validation (implemented but not yet active) |
| `auth/token_manager.rs` | JWT `exp` extraction; expiry checks |
| `startup.rs` | API key resolution helpers (implemented but not yet called from `main.rs`) |
| `tou/openei_client.rs` | Fetches TOU rate schedules from OpenEI URDB |
| `trueup/calculator.rs` | Net-metering annual cost/credit estimation; TOU period classification by rate ranking |
| `error.rs` | Typed error hierarchy (`AppError`, `GatewayError`, `StorageError`, etc.) with Axum `IntoResponse` |

### Database schema

Five tables (see `migrations/001_initial.sql`):
- `energy_window` — one row per completed 15-min interval (production, consumption, grid import/export in Wh)
- `microinverter_snapshot` — per-inverter watts + online status at each window boundary
- `tou_rate_schedule` — versioned TOU rate JSON from OpenEI; never deleted
- `true_up_estimate` — cached NEM true-up results (references `tou_rate_schedule.id`)
- `config_store` — key/value runtime state (last poll timestamp, cumulative Wh, last window start)

### Testing approach

- **Unit tests** live in `tests/unit/` and are declared in `tests/unit.rs`. They test pure logic: `window_aggregator`, `token_manager`, `trueup/calculator`, `inverter/snapshot`.
- **Integration tests** live in `tests/integration/` and are declared in `tests/integration.rs`. They spin up an in-memory SQLite pool, run migrations, construct an `AppState`, and call `create_router(...).oneshot(request)` directly — no network or spawned process needed.
- The crate is exposed as a library (`src/lib.rs`) so integration tests can import `enphase_bridge::api::server::{AppState, create_router}`.
- `mockito` is used in gateway client tests to stub HTTP responses without a real gateway.

## Specs

Active specs are in `specs/`. The plan for the current or most recent feature is the authoritative source for implementation decisions:
- `specs/001-enphase-gateway-api/` — core gateway polling + REST API
- `specs/002-api-key-auth/` — optional Bearer token auth (implemented; wiring into config/main is pending)

Each spec directory contains `spec.md`, `plan.md`, `data-model.md`, `research.md`, and `contracts/api.md`.
