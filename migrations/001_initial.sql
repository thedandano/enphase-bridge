-- Energy windows: one row per 15-minute interval
CREATE TABLE IF NOT EXISTS energy_window (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    window_start   INTEGER NOT NULL UNIQUE,
    wh_produced    REAL    NOT NULL,
    wh_consumed    REAL    NOT NULL,
    wh_grid_import REAL    NOT NULL,
    wh_grid_export REAL    NOT NULL,
    is_complete    INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_energy_window_start ON energy_window(window_start);

-- Per-microinverter snapshot at each 15-minute boundary
CREATE TABLE IF NOT EXISTS microinverter_snapshot (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    window_start  INTEGER NOT NULL,
    serial_number TEXT    NOT NULL,
    watts_output  REAL    NOT NULL,
    is_online     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_inverter_window ON microinverter_snapshot(serial_number, window_start);

-- Versioned SDGE TOU rate schedules from OpenEI — never deleted
CREATE TABLE IF NOT EXISTS tou_rate_schedule (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    fetched_at     INTEGER NOT NULL,
    effective_date TEXT,
    utility_name   TEXT    NOT NULL,
    rate_label     TEXT    NOT NULL,
    rate_json      TEXT    NOT NULL
);

-- Cached NEM true-up computation results
CREATE TABLE IF NOT EXISTS true_up_estimate (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    computed_at              INTEGER NOT NULL,
    period_start             INTEGER NOT NULL,
    period_end               INTEGER NOT NULL,
    net_cost_usd             REAL    NOT NULL,
    peak_import_kwh          REAL    NOT NULL,
    peak_export_kwh          REAL    NOT NULL,
    offpeak_import_kwh       REAL    NOT NULL,
    offpeak_export_kwh       REAL    NOT NULL,
    super_offpeak_import_kwh REAL    NOT NULL,
    super_offpeak_export_kwh REAL    NOT NULL,
    tou_schedule_id          INTEGER NOT NULL REFERENCES tou_rate_schedule(id)
);

-- Persistent runtime state (last window, cumulative values, serial cache)
CREATE TABLE IF NOT EXISTS config_store (
    key     TEXT    PRIMARY KEY,
    value   TEXT    NOT NULL,
    updated INTEGER NOT NULL
);
