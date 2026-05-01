use super::common::{
    OFF_PEAK_TS, PEAK_TS, SUMMER_OFF_PEAK_TS, SUMMER_PEAK_TS, SUMMER_SUPER_OP_TS, SUPER_OP_TS,
    WEEKEND_MIDDAY_TS,
};
use enphase_bridge::storage::models::{EnergyWindow, TouRateSchedule};
use enphase_bridge::trueup::calculator;

fn fixture_schedule() -> TouRateSchedule {
    TouRateSchedule {
        id: 1,
        fetched_at: 0,
        effective_date: None,
        utility_name: "Test Utility".into(),
        rate_label: "Test Rate".into(),
        rate_json: super::common::fixture_rate_json(),
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
        formula_version: 0,
        was_clamped: false,
        avg_production_w: None,
        avg_consumption_w: None,
        avg_grid_w: None,
    }
}

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

fn make_schedule(rate_json: &str) -> TouRateSchedule {
    TouRateSchedule {
        id: 1,
        fetched_at: 0,
        effective_date: None,
        utility_name: "Test".into(),
        rate_label: "Test".into(),
        rate_json: rate_json.to_string(),
    }
}

fn fixture_2period() -> TouRateSchedule {
    let row = "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,1,1,1,1,0,0,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.20,"unit":"kWh"}}],[{{"rate":0.40,"unit":"kWh"}}]]}}"#,
        r = months.join(","),
    );
    make_schedule(&rate_json)
}

fn fixture_4period() -> TouRateSchedule {
    // 4 periods: indices 0-3, sorted by rate
    // Row: 0=offpeak2, 1=super_offpeak, 2=offpeak1, 3=peak
    let row = "[1,1,1,1,1,1,2,2,2,2,2,2,2,2,2,2,3,3,3,3,3,2,2,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.35,"unit":"kWh"}}],[{{"rate":0.10,"unit":"kWh"}}],[{{"rate":0.25,"unit":"kWh"}}],[{{"rate":0.50,"unit":"kWh"}}]]}}"#,
        r = months.join(","),
    );
    make_schedule(&rate_json)
}

// 9.1 — 2-period schedule: no SuperOffPeak bucket, OffPeak captures all non-peak
#[test]
fn test_2_period_no_super_off_peak() {
    let schedule = fixture_2period();
    let windows = vec![
        make_window(SUPER_OP_TS, 100.0, 0.0), // hour 0 PST → period 0 (off-peak, lower rate)
        make_window(PEAK_TS, 200.0, 0.0),     // hour 16 PST → period 1 (peak, higher rate)
    ];
    let result = calculator::calculate(&schedule, &windows).unwrap();
    assert_eq!(result.super_off_peak.import_kwh, 0.0);
    assert!(result.off_peak.import_kwh > 0.0);
    assert!(result.peak.import_kwh > 0.0);
}

// 9.2 — 4-period schedule: highest→Peak, lowest→SuperOffPeak, middle 2→OffPeak
#[test]
fn test_4_period_all_buckets_represented() {
    let schedule = fixture_4period();
    // period 3 (rate 0.50) → peak; period 1 (rate 0.10) → super-off-peak; 0 and 2 → off-peak
    // PEAK_TS (hour 16 PST Monday) → period 3
    let windows = vec![
        make_window(PEAK_TS, 100.0, 0.0),     // hour 16 → period 3 (peak)
        make_window(OFF_PEAK_TS, 100.0, 0.0), // hour 12 → period 2 (off-peak)
        make_window(SUPER_OP_TS, 100.0, 0.0), // hour 0 → period 1 (super-off-peak)
    ];
    let result = calculator::calculate(&schedule, &windows).unwrap();
    assert!(result.peak.import_kwh > 0.0);
    assert!(result.off_peak.import_kwh > 0.0);
    assert!(result.super_off_peak.import_kwh > 0.0);
}

// 9.3 — tied rates: two calls produce identical output (stable sort)
#[test]
fn test_tied_rates_deterministic() {
    let row = "[0,0,0,0,0,0,0,0,0,0,0,0,1,1,1,1,1,1,1,1,1,1,1,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.30,"unit":"kWh"}}],[{{"rate":0.30,"unit":"kWh"}}]]}}"#,
        r = months.join(","),
    );
    let schedule = make_schedule(&rate_json);
    let windows = vec![make_window(PEAK_TS, 500.0, 0.0)];

    let r1 = calculator::calculate(&schedule, &windows).unwrap();
    let r2 = calculator::calculate(&schedule, &windows).unwrap();
    assert_eq!(r1.net_cost_usd, r2.net_cost_usd);
    assert_eq!(r1.peak.import_kwh, r2.peak.import_kwh);
    assert_eq!(r1.off_peak.import_kwh, r2.off_peak.import_kwh);
}

// 9.4 — missing "rate" key → Err(ParseError)
#[test]
fn test_missing_rate_key_returns_parse_error() {
    use enphase_bridge::error::{AppError, TouError};
    let row = "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"unit":"kWh"}}]]}}"#,
        r = months.join(","),
    );
    let schedule = make_schedule(&rate_json);
    let err = calculator::calculate(&schedule, &[make_window(PEAK_TS, 100.0, 0.0)]).unwrap_err();
    assert!(matches!(err, AppError::Tou(TouError::ParseError(_))));
}

// 9.5 — schedule with fewer than 12 months → Err(ParseError) when window falls in missing month
#[test]
fn test_schedule_fewer_than_12_months_returns_parse_error() {
    use enphase_bridge::error::{AppError, TouError};
    // Only 6 months — window in month 8 (September, 0-indexed) will be out of bounds
    let row = "[0,0,0,0,0,0,1,1,1,1,1,1,1,1,1,1,2,2,2,2,2,1,1,1]";
    let months_6: Vec<&str> = vec![row; 6];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.15}}],[{{"rate":0.25}}],[{{"rate":0.40}}]]}}"#,
        r = months_6.join(","),
    );
    let schedule = make_schedule(&rate_json);
    // UTC 2024-09-02 00:00:00 = PST 2024-09-01 17:00 → month index 8 (September)
    let sep_ts: i64 = 1725235200;
    let err = calculator::calculate(&schedule, &[make_window(sep_ts, 100.0, 0.0)]).unwrap_err();
    assert!(matches!(err, AppError::Tou(TouError::ParseError(_))));
}

// 9.7 — missing "sell" key: returns Ok and uses buy rate as sell rate
#[test]
fn test_missing_sell_key_returns_ok_with_buy_rate() {
    // No "sell" field — sell_rate should default to rate (0.40)
    let row = "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,2,2,2,2,0,0,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.15,"unit":"kWh"}}],[{{"rate":0.25,"unit":"kWh"}}],[{{"rate":0.40,"unit":"kWh"}}]]}}"#,
        r = months.join(","),
    );
    let schedule = make_schedule(&rate_json);
    // Export at peak hour → credit should use buy rate $0.40
    let windows = vec![make_window(PEAK_TS, 0.0, 500.0)];
    let result = calculator::calculate(&schedule, &windows).unwrap();
    assert!((result.peak.export_credit_usd - 0.5 * 0.40).abs() < 1e-6);
}

// 10.2 — real TOU-DR-2 fixture: all three periods classified correctly (regression)
#[test]
fn test_sdge_tou_dr2_fixture_all_periods() {
    let fixture_json = std::fs::read_to_string("tests/fixtures/sdge_tou_dr2_item.json")
        .expect("fixture file must exist");
    let schedule = TouRateSchedule {
        id: 1,
        fetched_at: 0,
        effective_date: None,
        utility_name: "San Diego Gas & Electric".into(),
        rate_label: "TOU-DR-2".into(),
        rate_json: fixture_json,
    };
    // PEAK_TS: UTC 2024-01-02 00:00:00 = PST 2024-01-01 16:00 (Monday, month 0) → period 2 (peak)
    let peak = calculator::calculate(&schedule, &[make_window(PEAK_TS, 500.0, 0.0)]).unwrap();
    assert!(peak.peak.import_kwh > 0.0, "Jan hour 16 should be Peak");
    assert_eq!(peak.off_peak.import_kwh, 0.0);
    assert_eq!(peak.super_off_peak.import_kwh, 0.0);

    // OFF_PEAK_TS: UTC 2024-01-02 20:00:00 = PST 2024-01-02 12:00 (Tuesday, month 0) → period 1
    let op = calculator::calculate(&schedule, &[make_window(OFF_PEAK_TS, 200.0, 0.0)]).unwrap();
    assert_eq!(op.peak.import_kwh, 0.0);
    assert!(
        op.off_peak.import_kwh > 0.0,
        "Jan hour 12 should be OffPeak"
    );
    assert_eq!(op.super_off_peak.import_kwh, 0.0);

    // SUPER_OP_TS: UTC 2024-01-02 08:00:00 = PST 2024-01-02 00:00 (Tuesday, month 0) → period 0
    let sop = calculator::calculate(&schedule, &[make_window(SUPER_OP_TS, 300.0, 0.0)]).unwrap();
    assert_eq!(sop.peak.import_kwh, 0.0);
    assert_eq!(sop.off_peak.import_kwh, 0.0);
    assert!(
        sop.super_off_peak.import_kwh > 0.0,
        "Jan hour 0 should be SuperOffPeak"
    );
}

fn coastal_fixture_schedule() -> TouRateSchedule {
    let fixture_json =
        std::fs::read_to_string("tests/fixtures/sdge_tou_dr_coastal_baseline_item.json")
            .expect("coastal baseline fixture must exist");
    TouRateSchedule {
        id: 2,
        fetched_at: 0,
        effective_date: None,
        utility_name: "San Diego Gas & Electric".into(),
        rate_label: "TOU-DR Coastal Baseline Region".into(),
        rate_json: fixture_json,
    }
}

// 3.1 — 6-period coastal baseline, winter month: hour 16 → Peak, hour 0 → SuperOffPeak, hour 12 → OffPeak
#[test]
fn test_coastal_6period_winter_classification() {
    let schedule = coastal_fixture_schedule();

    // Jan hour 16 → period 3 (rate 0.55, highest in winter group) → Peak
    let peak = calculator::calculate(&schedule, &[make_window(PEAK_TS, 100.0, 0.0)]).unwrap();
    assert!(
        peak.peak.import_kwh > 0.0,
        "Jan hour 16 (period 3) should be Peak"
    );
    assert_eq!(peak.off_peak.import_kwh, 0.0);
    assert_eq!(peak.super_off_peak.import_kwh, 0.0);

    // Jan hour 0 → period 5 (rate 0.35, lowest in winter group) → SuperOffPeak
    let sop = calculator::calculate(&schedule, &[make_window(SUPER_OP_TS, 100.0, 0.0)]).unwrap();
    assert_eq!(sop.peak.import_kwh, 0.0);
    assert_eq!(sop.off_peak.import_kwh, 0.0);
    assert!(
        sop.super_off_peak.import_kwh > 0.0,
        "Jan hour 0 (period 5) should be SuperOffPeak"
    );

    // Jan hour 12 → period 4 (rate 0.40, mid in winter group) → OffPeak
    let op = calculator::calculate(&schedule, &[make_window(OFF_PEAK_TS, 100.0, 0.0)]).unwrap();
    assert_eq!(op.peak.import_kwh, 0.0);
    assert!(
        op.off_peak.import_kwh > 0.0,
        "Jan hour 12 (period 4) should be OffPeak"
    );
    assert_eq!(op.super_off_peak.import_kwh, 0.0);
}

// 3.2 — 6-period coastal baseline, summer month: hour 16 → Peak, hour 0 → SuperOffPeak, hour 8 → OffPeak
#[test]
fn test_coastal_6period_summer_classification() {
    let schedule = coastal_fixture_schedule();

    // Jul hour 16 → period 0 (rate 0.60, highest in summer group) → Peak
    let peak =
        calculator::calculate(&schedule, &[make_window(SUMMER_PEAK_TS, 100.0, 0.0)]).unwrap();
    assert!(
        peak.peak.import_kwh > 0.0,
        "Jul hour 16 (period 0) should be Peak"
    );
    assert_eq!(peak.off_peak.import_kwh, 0.0);
    assert_eq!(peak.super_off_peak.import_kwh, 0.0);

    // Jul hour 0 → period 2 (rate 0.30, lowest in summer group) → SuperOffPeak
    let sop =
        calculator::calculate(&schedule, &[make_window(SUMMER_SUPER_OP_TS, 100.0, 0.0)]).unwrap();
    assert_eq!(sop.peak.import_kwh, 0.0);
    assert_eq!(sop.off_peak.import_kwh, 0.0);
    assert!(
        sop.super_off_peak.import_kwh > 0.0,
        "Jul hour 0 (period 2) should be SuperOffPeak"
    );

    // Jul hour 8 → period 1 (rate 0.45, mid in summer group) → OffPeak
    let op =
        calculator::calculate(&schedule, &[make_window(SUMMER_OFF_PEAK_TS, 100.0, 0.0)]).unwrap();
    assert_eq!(op.peak.import_kwh, 0.0);
    assert!(
        op.off_peak.import_kwh > 0.0,
        "Jul hour 8 (period 1) should be OffPeak"
    );
    assert_eq!(op.super_off_peak.import_kwh, 0.0);
}

// 3.3 — cross-season: same hour (16:00) in winter and summer both classify as Peak within their group
#[test]
fn test_coastal_cross_season_both_peak() {
    let schedule = coastal_fixture_schedule();
    // Jan hour 16 → period 3 → Peak (winter group, rate 0.55)
    // 100 Wh = 0.1 kWh × $0.55 = $0.055
    let winter = calculator::calculate(&schedule, &[make_window(PEAK_TS, 100.0, 0.0)]).unwrap();
    // Jul hour 16 → period 0 → Peak (summer group, rate 0.60)
    // 100 Wh = 0.1 kWh × $0.60 = $0.060
    let summer =
        calculator::calculate(&schedule, &[make_window(SUMMER_PEAK_TS, 100.0, 0.0)]).unwrap();
    assert!(winter.peak.import_kwh > 0.0, "Jan hour 16 should be Peak");
    assert!(summer.peak.import_kwh > 0.0, "Jul hour 16 should be Peak");
    assert_eq!(winter.off_peak.import_kwh, 0.0);
    assert_eq!(summer.off_peak.import_kwh, 0.0);
    // cost assertions
    assert!(
        (winter.peak.import_cost_usd - 0.055).abs() < 1e-9,
        "winter peak cost: expected $0.055, got {}",
        winter.peak.import_cost_usd
    );
    assert!(
        (summer.peak.import_cost_usd - 0.060).abs() < 1e-9,
        "summer peak cost: expected $0.060, got {}",
        summer.peak.import_cost_usd
    );
}

// 3.4 — weekend-only period: period appearing only in weekend rows is included in active set
#[test]
fn test_weekend_only_period_in_active_set() {
    // weekday uses {0,1}; weekend uses only {2}; union = {0,1,2}
    // rates: 0=0.25, 1=0.15, 2=0.10 → per-month: 0=Peak, 1=OffPeak, 2=SuperOffPeak
    let weekday_row = "[0,0,0,0,0,0,0,0,0,0,0,0,1,1,1,1,1,1,1,1,1,1,1,0]";
    let weekend_row = "[2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2]";
    let months_wd: Vec<&str> = vec![weekday_row; 12];
    let months_we: Vec<&str> = vec![weekend_row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{w}],"energyweekendschedule":[{e}],"energyratestructure":[[{{"rate":0.25,"unit":"kWh"}}],[{{"rate":0.15,"unit":"kWh"}}],[{{"rate":0.10,"unit":"kWh"}}]]}}"#,
        w = months_wd.join(","),
        e = months_we.join(","),
    );
    let schedule = make_schedule(&rate_json);
    // WEEKEND_MIDDAY_TS: Jan 7 (Sunday) 12:00 PST → period 2 (weekend-only) → SuperOffPeak
    let result =
        calculator::calculate(&schedule, &[make_window(WEEKEND_MIDDAY_TS, 100.0, 0.0)]).unwrap();
    assert_eq!(result.peak.import_kwh, 0.0);
    assert_eq!(result.off_peak.import_kwh, 0.0);
    assert!(
        result.super_off_peak.import_kwh > 0.0,
        "weekend-only period 2 should be SuperOffPeak via union"
    );
}

// 3.5 — n=1 active period: single index classifies as Peak; degenerate warning would be emitted
#[test]
fn test_single_active_period_classifies_as_peak() {
    // All hours use period 0 on both weekday and weekend
    let row = "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.30,"unit":"kWh"}}]]}}"#,
        r = months.join(","),
    );
    let schedule = make_schedule(&rate_json);
    let result = calculator::calculate(&schedule, &[make_window(PEAK_TS, 100.0, 0.0)]).unwrap();
    assert!(
        result.peak.import_kwh > 0.0,
        "single-period month should classify as Peak"
    );
    assert_eq!(result.off_peak.import_kwh, 0.0);
    assert_eq!(result.super_off_peak.import_kwh, 0.0);
}

// 3.6 — n=2 active periods: highest → Peak, other → OffPeak, no SuperOffPeak
#[test]
fn test_two_active_periods_no_super_off_peak() {
    // Same as fixture_2period — restate explicitly for clarity
    let row = "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,1,1,1,1,0,0,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.15,"unit":"kWh"}}],[{{"rate":0.40,"unit":"kWh"}}]]}}"#,
        r = months.join(","),
    );
    let schedule = make_schedule(&rate_json);
    let peak = calculator::calculate(&schedule, &[make_window(PEAK_TS, 100.0, 0.0)]).unwrap();
    let off = calculator::calculate(&schedule, &[make_window(SUPER_OP_TS, 100.0, 0.0)]).unwrap();
    assert!(peak.peak.import_kwh > 0.0);
    assert_eq!(peak.super_off_peak.import_kwh, 0.0);
    assert!(off.off_peak.import_kwh > 0.0);
    assert_eq!(off.super_off_peak.import_kwh, 0.0);
}

// 3.8 — period index in schedule but missing from energyratestructure → explicit ParseError
#[test]
fn test_schedule_references_missing_period_index_returns_error() {
    use enphase_bridge::error::{AppError, TouError};
    // Schedule references period index 3, but energyratestructure only has periods 0-2
    let row = "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,3,3,3,3,3,0,0,0]";
    let months: Vec<&str> = vec![row; 12];
    let rate_json = format!(
        r#"{{"energyweekdayschedule":[{r}],"energyweekendschedule":[{r}],"energyratestructure":[[{{"rate":0.15}}],[{{"rate":0.25}}],[{{"rate":0.40}}]]}}"#,
        r = months.join(","),
    );
    let schedule = make_schedule(&rate_json);
    let err = calculator::calculate(&schedule, &[make_window(PEAK_TS, 100.0, 0.0)]).unwrap_err();
    assert!(matches!(err, AppError::Tou(TouError::ParseError(_))));
}

// 3.9 — DST spring-forward boundary: timestamps straddling the clocks-forward moment both classify
//        correctly without crashing
#[test]
fn test_dst_spring_forward_boundary() {
    // 2024-03-08 09:30 UTC = 01:30 AM PST Friday (before spring forward): month 2, hour 1 → period 0 → SuperOffPeak
    const DST_SPRING_PRE_TS: i64 = 1709890200;
    // 2024-03-11 10:30 UTC = 03:30 AM PDT Monday (after spring forward): month 2, hour 3 → period 0 → SuperOffPeak
    const DST_SPRING_POST_TS: i64 = 1710153000;

    let schedule = fixture_schedule();
    // hours 0-5 → period 0 (super-off-peak) in fixture_rate_json

    let pre = calculator::calculate(&schedule, &[make_window(DST_SPRING_PRE_TS, 100.0, 0.0)])
        .expect("pre-DST timestamp must not error");
    let post = calculator::calculate(&schedule, &[make_window(DST_SPRING_POST_TS, 100.0, 0.0)])
        .expect("post-DST timestamp must not error");

    assert!(
        pre.super_off_peak.import_kwh > 0.0,
        "01:30 PST (hour 1) should be SuperOffPeak"
    );
    assert_eq!(pre.peak.import_kwh, 0.0, "01:30 PST should not be Peak");

    assert!(
        post.super_off_peak.import_kwh > 0.0,
        "03:30 PDT (hour 3) should be SuperOffPeak"
    );
    assert_eq!(post.peak.import_kwh, 0.0, "03:30 PDT should not be Peak");
}
