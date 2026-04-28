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
IQ Gateway (HTTPS + JWT)
    → GatewayClient        (collector/gateway_client.rs)
    → WindowAggregator     (collector/window_aggregator.rs)  — floor-divides timestamps into 15-min buckets
    → Scheduler            (collector/scheduler.rs)          — detects window boundary crossings; writes windows + snapshots
    → SQLite (sqlx)        (storage/)
    → Axum handlers        (api/handlers/)                   — query-only reads
```

The scheduler persists the last cumulative reading (timestamp + Wh) to the `config_store` table so it survives restarts without re-polling.

### Key design decisions

- **Cumulative-to-delta conversion**: The gateway exposes lifetime `actEnergyDlvd` counters. `window_aggregator.rs` computes deltas between consecutive readings. Grid import/export is derived from the energy balance (`produced - consumed`).
- **Single SQLite connection** (`max_connections(1)`) with WAL mode — avoids write contention while allowing concurrent reads.
- **Optional Bearer auth** — disabled by default. When enabled (`api.require_auth = true`), `api_key_middleware` runs on all routes except `/api/health`. Keys are validated with constant-time comparison (`subtle::ConstantTimeEq`). Auto-generated keys (when no `api_key` is set) are written to **stderr only**, not to the structured log, to avoid leaking into log aggregators.
- **Gateway JWT is not verified** — `token_manager.rs` only decodes the `exp` claim to warn/fail on expiry. The gateway's ES256 signature is not validated (the gateway accepts it as-is).
- **Config layering**: `figment` merges `config.toml` → `ENPHASE__` environment variables. Section separator is `__` (e.g. `ENPHASE__API__PORT=9090`).

### Module map

| Module | Responsibility |
|--------|---------------|
| `config.rs` | Figment-based config loading (TOML + env) |
| `collector/gateway_client.rs` | HTTPS client for IQ Gateway metering + inverter endpoints |
| `collector/window_aggregator.rs` | 15-min window math: `window_boundary()`, `compute_delta()` |
| `collector/scheduler.rs` | Polling loop; detects window crossings; persists via `config_store` |
| `storage/db.rs` | SQLite connection pool setup + sqlx migrations |
| `storage/models.rs` | Shared data structs (`EnergyWindow`, `InverterSnapshot`, etc.) |
| `storage/{energy_window,inverter_snapshot,tou_schedule,true_up,config_store}.rs` | Per-table query functions |
| `api/server.rs` | Axum router + `AppState` definition |
| `api/handlers/` | One file per route group: `energy`, `inverters`, `arrays`, `tou`, `trueup`, `health` |
| `api/middleware/api_key.rs` | Bearer token middleware; key generation + validation |
| `auth/token_manager.rs` | JWT `exp` extraction; expiry checks |
| `tou/openei_client.rs` | Fetches TOU rate schedules from OpenEI URDB |
| `trueup/calculator.rs` | Net-metering annual cost/credit estimation |
| `error.rs` | Typed error hierarchy (`AppError`, `GatewayError`, `StorageError`, etc.) with Axum `IntoResponse` |
| `startup.rs` | Startup validation helpers (API key resolution, fatal error types) |

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

## Specs

Active specs are in `specs/`. The plan for the current or most recent feature is the authoritative source for implementation decisions:
- `specs/001-enphase-gateway-api/` — core gateway polling + REST API
- `specs/002-api-key-auth/` — optional Bearer token auth

Each spec directory contains `spec.md`, `plan.md`, `data-model.md`, `research.md`, and `contracts/api.md`.
