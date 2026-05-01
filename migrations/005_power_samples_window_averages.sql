CREATE TABLE power_sample (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    sampled_at    INTEGER NOT NULL,
    production_w  REAL    NOT NULL,
    consumption_w REAL    NOT NULL,
    grid_w        REAL    NOT NULL
);
CREATE INDEX idx_power_sample_at ON power_sample(sampled_at);

ALTER TABLE energy_window ADD COLUMN avg_production_w REAL;
ALTER TABLE energy_window ADD COLUMN avg_consumption_w REAL;
ALTER TABLE energy_window ADD COLUMN avg_grid_w REAL;
