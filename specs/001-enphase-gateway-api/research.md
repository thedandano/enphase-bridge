# Research: Enphase Gateway Data Service

**Branch**: `001-enphase-gateway-api` | **Date**: 2026-04-26
**Phase**: 0 — Pre-design research

---

## 1. Enphase IQ Gateway Local API

### Decision: Use `/ivp/meters/readings` for aggregate data, `/ivp/peb/devstatus` for per-inverter

**Rationale**: `/ivp/meters/readings` responds in ~64ms and returns current watt-level production, consumption, and grid flow. It supersedes the legacy `/production.json` endpoint (~2500ms, deprecated for real-time use). Per-inverter power comes from `/ivp/peb/devstatus` which returns milliwatt values per device.

**Alternatives considered**: `/production.json` — rejected due to latency and deprecation trajectory.

### Key Endpoints

| Endpoint | Purpose | Latency |
|----------|---------|---------|
| `GET /ivp/meters/readings` | Aggregate production + consumption + grid flow | ~64ms |
| `GET /ivp/peb/devstatus` | Per-microinverter power output (milliwatts) | ~fast |
| `GET /inventory.json` | Device list — serial numbers for all microinverters | one-time |
| `GET /production.json` | Legacy aggregate (deprecated — do not use) | ~2500ms |

### Response Shape: `/ivp/meters/readings`

Returns an array of meter objects. Key fields per object:
- `actEnergyDlvd` — watt-hours exported to grid (lifetime cumulative)
- `actEnergyRcvd` — watt-hours imported from grid (lifetime cumulative)
- `wNow` — current watts (instantaneous)
- `channels[]` — per-phase breakdown

**Deriving 15-minute watt-hours**: Record `actEnergyDlvd` and `actEnergyRcvd` at window start and end; compute delta to get watt-hours for that interval. Record `wNow` samples throughout for average watt calculation.

### Response Shape: `/ivp/peb/devstatus`

Returns a `pcu` object with:
- `pcu.fields[]` — array of field names (column headers)
- `pcu.values[][]` — 2D array; each row = one inverter's readings
- Field `acPowerINmW` = instantaneous AC power in milliwatts → divide by 1000 for watts

Serial numbers come from `inventory.json` and are correlated to `devstatus` rows by position/serial field.

**Gateway-to-inverter refresh rate**: Every 6–7 minutes. Polling faster than this returns stale data for individual inverter readings. At 15-minute windows this is not a concern.

### Authentication

- **Token source**: Enphase Enlighten cloud (`entrez.enphaseenergy.com/entrez_tokens`)
- **Token lifetime**: 1 year for system owners (homeowner account)
- **Header**: `Authorization: Bearer <JWT>`
- **Gateway TLS**: Self-signed certificate — HTTP client must disable cert verification for local gateway calls
- **Token fields**: Standard JWT with `iat`, `exp`, `jti`

**Implementation note**: Parse `exp` claim from JWT to determine refresh timing. With 1-year tokens, refresh is infrequent but must be handled. Store token + expiry in config/secrets file, not in the database.

### Polling Strategy

- Poll aggregate readings every 60 seconds (configurable, minimum 15s)
- Snapshot per-inverter data at each 15-minute boundary
- At 15-minute boundary: compute window delta from `actEnergyDlvd`/`actEnergyRcvd` values and store `EnergyWindow`; also store `MicroinverterSnapshot` for each inverter
- Cloud itself reports every 15 minutes — our windows align with Enphase's own reporting cadence

---

## 2. OpenEI Utility Rate Database (URDB) API

### Decision: `https://api.openei.org/utility_rates` with v7 JSON format

**Rationale**: Free, self-service API key, confirmed SDGE rates present, v7 is the latest stable version. The `developer.nlr.gov` URL is the documentation portal; the actual API is at `api.openei.org`.

**Alternatives considered**: MIDAS API (CA-specific but focused on demand response, not NEM tariffs), RateAcuity (paid), manual config file (rejected in favour of authoritative data source).

### Query

```
GET https://api.openei.org/utility_rates
  ?version=7
  &format=json
  &api_key=<KEY>
  &ratesforutility=San+Diego+Gas+%26+Electric
  &sector=Residential
  &detail=full
```

Pagination: `limit` (max 500) and `offset` parameters.

### Response Schema (key TOU fields)

```json
{
  "utility": "San Diego Gas & Electric",
  "label": "...",
  "effective_date": "YYYY-MM-DD",
  "energyratestructure": [
    [{"rate": 0.XX, "unit": "kWh"}]
  ],
  "energyweekdayschedule": [[period_index_per_hour_0..23]],
  "energyweekendschedule": [[period_index_per_hour_0..23]],
  "flatdistributiongenerationrate": [...]
}
```

**Period mapping**: `energyweekdayschedule[month][hour]` gives an index into `energyratestructure` for the rate. Off-peak, peak, super-off-peak are expressed as different indices.

**Strategy**: Fetch all residential SDG&E rates, filter for the active NEM tariff (e.g. "NEM 2.0 TOU-DR" or current NEM 3.0 schedule), store raw JSON blob as `TOURateSchedule.rate_json`, parse at query time for true-up computation.

**Rate schedule refresh**: Fetch on first run and whenever the service operator runs the `/api/tou/refresh` endpoint. Store with `effective_date`; retain all historical versions for reproducible estimates.

---

## 3. Rust Technology Stack

### Decision: tokio + axum + sqlx (SQLite) + reqwest + tracing + serde + jsonwebtoken + figment

**Rationale**: All production-ready, async-first, actively maintained as of 2026. Minimal bloat for a home server daemon.

| Role | Crate | Version | Rationale |
|------|-------|---------|-----------|
| Async runtime | `tokio` | 1.40+ | Battle-tested, ecosystem anchor |
| HTTP server | `axum` | 0.8+ | Lightweight, tower-composable, correct for read-only local API |
| HTTP client | `reqwest` | 0.12+ | Ergonomic, tokio-native, TLS/cert control |
| Storage | `sqlx` + `sqlite` | 0.8+ | Compile-time query verification, async, WAL support |
| JWT | `jsonwebtoken` | 9.3+ | Mature, built-in expiry validation |
| Config | `figment` | 0.10+ | TOML + env var merge, strong type safety |
| Logging | `tracing` + `tracing-subscriber` | 0.1.40+ | Async-aware, JSON/logfmt formatters |
| Serialization | `serde` + `serde_json` | 1.0 | Universal |

**Alternative noted**: `rusqlite` for SQLite if async is not needed in storage layer — rejected in favour of `sqlx` for consistency and compile-time safety.

### reqwest TLS note

Gateway uses a self-signed cert. Configure reqwest client for gateway calls with `.danger_accept_invalid_certs(true)`. Use a **separate** reqwest client for external calls (OpenEI) with normal cert validation. Never mix the two client instances.

---

## 4. SQLite Schema and Storage Design

### Decision: WAL mode, single writer pool, JSON blob for TOU schedules, delta-based energy windows

**Rationale**: WAL mode allows concurrent reads during write (critical for API serving while daemon writes). Single connection for writes prevents lock contention. JSON blob for TOU schedule avoids over-normalization of rarely-queried rate data.

### Index Strategy

- `energy_window(window_start)` — primary time-range index; all queries filter or sort by this
- `microinverter_snapshot(serial_number, window_start)` — composite index; queries group by inverter then slice time
- No index on `tou_rate_schedule` — few rows, scanned directly

### Data Retention

- Keep rolling 13 months of `energy_window` and `microinverter_snapshot` (covers one full NEM anniversary cycle plus one month buffer)
- Archive older records to `energy_archive.db` rather than deleting (preserves data for ad-hoc queries)
- `tou_rate_schedule` rows are never deleted — small table, all versions needed for reproducible historical estimates
- Prune on service startup and weekly thereafter

### Connection Pool Config

```rust
SqlitePoolOptions::new()
  .max_connections(1)          // Single writer
  .connect_with(
    SqliteConnectOptions::new()
      .filename("energy.db")
      .create_if_missing(true)
      .journal_mode(SqliteJournalMode::Wal)
      .synchronous(SqliteSynchronous::Normal)
      .foreign_keys(true)
  )
```

---

## All NEEDS CLARIFICATION items resolved

No items from Technical Context remain unresolved. Proceeding to Phase 1.
