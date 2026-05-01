-- Add formula_version to track which formula computed each window.
-- formula_version = 0: permanently unrecomputable (pre-boundary_snapshot era rows, or JSON-oversized)
-- formula_version > 0 AND < CURRENT: stale, recomputable from boundary_snapshot
-- formula_version = CURRENT: computed by the current formula
ALTER TABLE energy_window ADD COLUMN formula_version INTEGER NOT NULL DEFAULT 0;

-- Backfill pre-fix rows as permanently unrecomputable (no anchor exists)
UPDATE energy_window SET formula_version = 0 WHERE window_start < 1746057600;

-- Raw cumulative gateway readings at each 15-min boundary crossing.
-- Enables retroactive recomputation of energy_window wh_* fields when formula bugs are found.
CREATE TABLE IF NOT EXISTS boundary_snapshot (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    window_start      INTEGER NOT NULL UNIQUE,
    production_wh     REAL    NOT NULL,
    grid_import_cum_wh REAL   NOT NULL,
    grid_export_cum_wh REAL   NOT NULL,
    captured_at       INTEGER NOT NULL,
    raw_meters_json   TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_boundary_snapshot_window ON boundary_snapshot(window_start);
