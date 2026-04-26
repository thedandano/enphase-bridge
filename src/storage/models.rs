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
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MicroinverterSnapshot {
    pub id: i64,
    pub window_start: i64,
    pub serial_number: String,
    pub watts_output: f64,
    pub is_online: bool,
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

#[allow(dead_code)]
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
