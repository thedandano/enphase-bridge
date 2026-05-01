use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct EnergyWindow {
    pub id: i64,
    pub window_start: i64,
    pub wh_produced: f64,
    pub wh_consumed: f64,
    pub wh_grid_import: f64,
    pub wh_grid_export: f64,
    pub is_complete: bool,
    pub formula_version: i32,
    pub was_clamped: bool,
    pub avg_production_w: Option<f64>,
    pub avg_consumption_w: Option<f64>,
    pub avg_grid_w: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BoundarySnapshot {
    pub id: i64,
    pub window_start: i64,
    pub production_wh: f64,
    pub grid_import_cum_wh: f64,
    pub grid_export_cum_wh: f64,
    pub captured_at: i64,
    pub raw_meters_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MicroinverterSnapshot {
    pub id: i64,
    pub window_start: i64,
    pub serial_number: String,
    pub watts_output: f64,
    pub is_online: bool,
    pub last_report_date: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TouRateSchedule {
    pub id: i64,
    pub fetched_at: i64,
    pub effective_date: Option<String>,
    pub utility_name: String,
    pub rate_label: String,
    pub rate_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PowerSample {
    pub id: i64,
    pub sampled_at: i64,
    pub production_w: f64,
    pub consumption_w: f64,
    pub grid_w: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PhaseReading {
    pub id: i64,
    pub sampled_at: i64,
    pub meter_eid: i64,
    pub channel_eid: i64,
    pub active_power_w_at_boundary: f64,
    pub energy_dlvd_wh: f64,
    pub energy_rcvd_wh: f64,
}
