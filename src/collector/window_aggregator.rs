use crate::storage::models::EnergyWindow;

pub const WINDOW_SECS: i64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct CumulativeReading {
    pub timestamp: i64,
    /// Lifetime watt-hours produced by solar (actEnergyDlvd on production meter)
    pub production_wh: f64,
    /// Lifetime watt-hours consumed by home (actEnergyDlvd on consumption meter)
    pub consumption_wh: f64,
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
    let wh_consumed = (curr.consumption_wh - prev.consumption_wh).max(0.0);
    // Energy balance: excess production → grid export; shortfall → grid import.
    let wh_grid_export = (wh_produced - wh_consumed).max(0.0);
    let wh_grid_import = (wh_consumed - wh_produced).max(0.0);

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
