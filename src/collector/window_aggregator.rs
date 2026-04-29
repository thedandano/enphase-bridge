use crate::storage::models::EnergyWindow;

pub const WINDOW_SECS: i64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct CumulativeReading {
    pub timestamp: i64,
    /// Lifetime watt-hours produced by solar (actEnergyDlvd on production meter)
    pub production_wh: f64,
    /// Lifetime watt-hours imported from grid (actEnergyDlvd on net meter)
    pub grid_import_cum_wh: f64,
    /// Lifetime watt-hours exported to grid (actEnergyRcvd on net meter)
    pub grid_export_cum_wh: f64,
}

/// Floor a Unix timestamp to the start of its 15-minute window.
pub fn window_boundary(ts: i64) -> i64 {
    (ts / WINDOW_SECS) * WINDOW_SECS
}

/// Compute the energy delta for a completed 15-minute window.
/// `id` is left as 0 — SQLite assigns it on insert.
pub fn compute_delta(
    window_start: i64,
    prev: &CumulativeReading,
    curr: &CumulativeReading,
    is_complete: bool,
) -> EnergyWindow {
    let wh_produced = (curr.production_wh - prev.production_wh).max(0.0);
    let wh_grid_import = (curr.grid_import_cum_wh - prev.grid_import_cum_wh).max(0.0);
    let wh_grid_export = (curr.grid_export_cum_wh - prev.grid_export_cum_wh).max(0.0);
    // Consumption derived from energy balance: produced + imported - exported
    let wh_consumed = (wh_produced + wh_grid_import - wh_grid_export).max(0.0);

    EnergyWindow {
        id: 0,
        window_start,
        wh_produced,
        wh_consumed,
        wh_grid_import,
        wh_grid_export,
        is_complete,
    }
}
