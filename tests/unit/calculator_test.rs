use enphase_bridge::storage::models::{EnergyWindow, TouRateSchedule};
use enphase_bridge::trueup::calculator;

// 3-period fixture: period 0 = super-off-peak ($0.15), period 1 = off-peak ($0.25), period 2 = peak ($0.40)
// Weekday schedule: hours 0-5 → 0, hours 6-15 → 1, hours 16-20 → 2, hours 21-23 → 1
// Weekend schedule: all hours → 1
fn fixture_schedule() -> TouRateSchedule {
    let weekday_row = "[0,0,0,0,0,0,1,1,1,1,1,1,1,1,1,1,2,2,2,2,2,1,1,1]";
    let weekend_row = "[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]";
    let months: Vec<&str> = vec![weekday_row; 12];
    let weekend_months: Vec<&str> = vec![weekend_row; 12];
    let rate_json = format!(
        r#"{{
            "energyweekdayschedule": [{weekday}],
            "energyweekendschedule": [{weekend}],
            "energyratestructure": [
                [{{"rate": 0.15, "unit": "kWh"}}],
                [{{"rate": 0.25, "unit": "kWh"}}],
                [{{"rate": 0.40, "unit": "kWh"}}]
            ]
        }}"#,
        weekday = months.join(","),
        weekend = weekend_months.join(","),
    );
    TouRateSchedule {
        id: 1,
        fetched_at: 0,
        effective_date: None,
        utility_name: "Test Utility".into(),
        rate_label: "Test Rate".into(),
        rate_json,
    }
}

fn make_window(window_start: i64, import_wh: f64, export_wh: f64) -> EnergyWindow {
    EnergyWindow {
        id: 0,
        window_start,
        wh_produced: 0.0,
        wh_consumed: 0.0,
        wh_grid_import: import_wh,
        wh_grid_export: export_wh,
        is_complete: true,
    }
}

// UTC 2024-01-02 00:00:00 = PST 2024-01-01 16:00 (Monday) → peak (period 2)
const PEAK_TS: i64 = 1704153600;

// UTC 2024-01-02 08:00:00 = PST 2024-01-02 00:00 (Tuesday) → super-off-peak (period 0)
const SUPER_OP_TS: i64 = 1704182400;

// UTC 2024-01-02 20:00:00 = PST 2024-01-02 12:00 (Tuesday) → off-peak (period 1)
const OFF_PEAK_TS: i64 = 1704225600;

#[test]
fn test_peak_import_classified_correctly() {
    let schedule = fixture_schedule();
    let windows = vec![make_window(PEAK_TS, 500.0, 0.0)];
    let result = calculator::calculate(&schedule, &windows).unwrap();

    assert!((result.peak.import_kwh - 0.5).abs() < 1e-6);
    assert!((result.peak.import_cost_usd - 0.20).abs() < 1e-6);
    assert_eq!(result.off_peak.import_kwh, 0.0);
    assert_eq!(result.super_off_peak.import_kwh, 0.0);
}

#[test]
fn test_super_off_peak_export_classified_correctly() {
    let schedule = fixture_schedule();
    let windows = vec![make_window(SUPER_OP_TS, 0.0, 300.0)];
    let result = calculator::calculate(&schedule, &windows).unwrap();

    assert!((result.super_off_peak.export_kwh - 0.3).abs() < 1e-6);
    assert!((result.super_off_peak.export_credit_usd - 0.045).abs() < 1e-6);
    assert_eq!(result.peak.export_kwh, 0.0);
    assert_eq!(result.off_peak.export_kwh, 0.0);
}

#[test]
fn test_net_cost_across_all_periods() {
    let schedule = fixture_schedule();
    let windows = vec![
        make_window(PEAK_TS, 500.0, 0.0),     // peak import 0.5 kWh → $0.20
        make_window(SUPER_OP_TS, 0.0, 300.0), // super-op export 0.3 kWh → -$0.045
        make_window(OFF_PEAK_TS, 200.0, 500.0), // off-peak: import 0.2 → $0.05, export 0.5 → -$0.125
    ];
    let result = calculator::calculate(&schedule, &windows).unwrap();

    // net = 0.20 + 0.05 - 0.045 - 0.125 = 0.08
    assert!((result.net_cost_usd - 0.08).abs() < 1e-6);
    assert!((result.off_peak.import_cost_usd - 0.05).abs() < 1e-6);
    assert!((result.off_peak.export_credit_usd - 0.125).abs() < 1e-6);
}

#[test]
fn test_empty_windows_returns_zero_cost() {
    let schedule = fixture_schedule();
    let result = calculator::calculate(&schedule, &[]).unwrap();
    assert_eq!(result.net_cost_usd, 0.0);
    assert_eq!(result.peak.import_kwh, 0.0);
}

#[test]
fn test_invalid_rate_json_returns_error() {
    let bad_schedule = TouRateSchedule {
        id: 1,
        fetched_at: 0,
        effective_date: None,
        utility_name: "x".into(),
        rate_label: "x".into(),
        rate_json: "not json".into(),
    };
    assert!(calculator::calculate(&bad_schedule, &[]).is_err());
}
