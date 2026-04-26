# Data Model: Enphase Gateway Data Service

**Branch**: `001-enphase-gateway-api` | **Date**: 2026-04-26
**Source**: spec.md entities + research.md decisions

---

## SQLite Schema

### `energy_window`

One row per 15-minute interval. Values are **delta watt-hours** for the window (not lifetime cumulative).

```sql
CREATE TABLE energy_window (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    window_start    INTEGER NOT NULL UNIQUE,  -- Unix timestamp (UTC), start of 15-min window
    wh_produced     REAL    NOT NULL,         -- Solar watt-hours generated this window
    wh_consumed     REAL    NOT NULL,         -- Home watt-hours consumed this window
    wh_grid_import  REAL    NOT NULL,         -- Watt-hours imported from grid this window
    wh_grid_export  REAL    NOT NULL,         -- Watt-hours exported to grid this window
    is_complete     INTEGER NOT NULL DEFAULT 1  -- 0 if any poll in window failed
);

CREATE INDEX idx_energy_window_start ON energy_window(window_start);
```

**Derivation**: At each 15-minute boundary, record the gateway's cumulative `actEnergyDlvd` / `actEnergyRcvd`. Subtract the previous window's cumulative values to get the delta for that window. `wNow` samples during the window inform whether `is_complete = 1`.

---

### `microinverter_snapshot`

One row per inverter per 15-minute boundary. Snapshot taken at window close.

```sql
CREATE TABLE microinverter_snapshot (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    window_start    INTEGER NOT NULL,   -- FK reference to energy_window.window_start
    serial_number   TEXT    NOT NULL,   -- Microinverter serial (from inventory.json)
    watts_output    REAL    NOT NULL,   -- Instantaneous AC watts at snapshot time
    is_online       INTEGER NOT NULL    -- 1 = reporting, 0 = absent from devstatus response
);

CREATE INDEX idx_inverter_window ON microinverter_snapshot(serial_number, window_start);
```

**Source**: `/ivp/peb/devstatus` → `pcu.values` rows → `acPowerINmW` field ÷ 1000. Serial numbers resolved from `/inventory.json` at startup and cached. An inverter absent from the `devstatus` response is recorded with `is_online = 0` and `watts_output = 0.0`.

---

### `tou_rate_schedule`

Versioned SDGE TOU rate schedules fetched from OpenEI URDB. Stored as a JSON blob; never deleted.

```sql
CREATE TABLE tou_rate_schedule (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    fetched_at      INTEGER NOT NULL,   -- Unix timestamp when fetched from OpenEI
    effective_date  TEXT,               -- "YYYY-MM-DD" from OpenEI response (nullable)
    utility_name    TEXT    NOT NULL,   -- e.g. "San Diego Gas & Electric"
    rate_label      TEXT    NOT NULL,   -- e.g. "EV-TOU-5" or "NEM 2.0 TOU-DR"
    rate_json       TEXT    NOT NULL    -- Full OpenEI response JSON for this rate
);
```

**Notes**:
- On `/api/tou/refresh`, a new row is inserted — old rows are never updated or deleted
- The service uses the row with the highest `effective_date` that is ≤ the window's `window_start` date when computing true-up, ensuring historical accuracy
- Multiple rate schedules may exist simultaneously (different SDGE tariffs) — query filters by `rate_label` matching the configured tariff name

---

### `true_up_estimate`

Cached result of a NEM true-up computation. Re-computed on demand; not auto-updated.

```sql
CREATE TABLE true_up_estimate (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    computed_at         INTEGER NOT NULL,  -- Unix timestamp of computation
    period_start        INTEGER NOT NULL,  -- Unix timestamp (UTC)
    period_end          INTEGER NOT NULL,  -- Unix timestamp (UTC)
    net_cost_usd        REAL    NOT NULL,  -- Positive = owe money, negative = credit
    peak_import_kwh     REAL    NOT NULL,
    peak_export_kwh     REAL    NOT NULL,
    offpeak_import_kwh  REAL    NOT NULL,
    offpeak_export_kwh  REAL    NOT NULL,
    super_offpeak_import_kwh REAL NOT NULL,
    super_offpeak_export_kwh REAL NOT NULL,
    tou_schedule_id     INTEGER NOT NULL   -- FK to tou_rate_schedule.id
);
```

**Notes**:
- Not a persistent cached layer — generated fresh on each API call (result stored for audit/history)
- `period_start` and `period_end` align to the NEM anniversary date configured by the user

---

### `config_store`

Key-value store for persistent runtime state (token, expiry, inverter inventory cache).

```sql
CREATE TABLE config_store (
    key     TEXT PRIMARY KEY,
    value   TEXT NOT NULL,
    updated INTEGER NOT NULL  -- Unix timestamp
);
```

**Keys used**:
- `gateway_token` — current JWT (stored encrypted or in secrets file, not in DB — this table is for non-sensitive state only)
- `gateway_token_expiry` — Unix timestamp of JWT expiry
- `inverter_serials` — JSON array of known serial numbers (refreshed from inventory.json on startup)
- `last_window_start` — Unix timestamp of last completed window (for delta computation)
- `last_cumulative_produced` — last known `actEnergyDlvd` cumulative value

---

## Entity Relationships

```
energy_window (1) ──< (N) microinverter_snapshot
                          [joined on window_start]

tou_rate_schedule (1) ──< (N) true_up_estimate
                              [FK: tou_schedule_id]
```

---

## Rust Type Mapping

```rust
// src/storage/models.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct EnergyWindow {
    pub id: i64,
    pub window_start: i64,       // Unix timestamp
    pub wh_produced: f64,
    pub wh_consumed: f64,
    pub wh_grid_import: f64,
    pub wh_grid_export: f64,
    pub is_complete: bool,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct MicroinverterSnapshot {
    pub id: i64,
    pub window_start: i64,
    pub serial_number: String,
    pub watts_output: f64,
    pub is_online: bool,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct TouRateSchedule {
    pub id: i64,
    pub fetched_at: i64,
    pub effective_date: Option<String>,
    pub utility_name: String,
    pub rate_label: String,
    pub rate_json: String,       // Raw OpenEI JSON
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct TrueUpEstimate {
    pub id: i64,
    pub computed_at: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub net_cost_usd: f64,
    pub peak_import_kwh: f64,
    pub peak_export_kwh: f64,
    pub offpeak_import_kwh: f64,
    pub offpeak_export_kwh: f64,
    pub super_offpeak_import_kwh: f64,
    pub super_offpeak_export_kwh: f64,
    pub tou_schedule_id: i64,
}
```
