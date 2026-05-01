CREATE TABLE IF NOT EXISTS phase_reading (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    sampled_at               INTEGER NOT NULL,
    meter_eid                INTEGER NOT NULL,
    channel_eid              INTEGER NOT NULL,
    active_power_w_at_boundary REAL NOT NULL,
    energy_dlvd_wh           REAL NOT NULL,
    energy_rcvd_wh           REAL NOT NULL,
    UNIQUE(sampled_at, meter_eid, channel_eid)
);
CREATE INDEX IF NOT EXISTS idx_phase_reading_sampled ON phase_reading(sampled_at, meter_eid);
