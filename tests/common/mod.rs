#![allow(dead_code)]

// UTC 2024-01-02 00:00:00 = PST 2024-01-01 16:00 (Monday) → peak (period 2)
pub const PEAK_TS: i64 = 1704153600;

// UTC 2024-01-02 08:00:00 = PST 2024-01-02 00:00 (Tuesday) → super-off-peak (period 0)
pub const SUPER_OP_TS: i64 = 1704182400;

// UTC 2024-01-02 20:00:00 = PST 2024-01-02 12:00 (Tuesday) → off-peak (period 1)
pub const OFF_PEAK_TS: i64 = 1704225600;

// UTC 2024-07-01 23:00:00 = PDT 2024-07-01 16:00 (Monday, month 6, hour 16) → summer peak
pub const SUMMER_PEAK_TS: i64 = 1719874800;

// UTC 2024-07-02 07:00:00 = PDT 2024-07-02 00:00 (Tuesday, month 6, hour 0) → summer super-off-peak
pub const SUMMER_SUPER_OP_TS: i64 = 1719903600;

// UTC 2024-07-02 15:00:00 = PDT 2024-07-02 08:00 (Tuesday, month 6, hour 8) → summer off-peak
pub const SUMMER_OFF_PEAK_TS: i64 = 1719932400;

// UTC 2024-01-07 20:00:00 = PST 2024-01-07 12:00 (Sunday, month 0, hour 12) → weekend midday
pub const WEEKEND_MIDDAY_TS: i64 = 1704657600;

// 3-period fixture: period 0 = super-off-peak ($0.15), period 1 = off-peak ($0.25), period 2 = peak ($0.40)
// Weekday schedule: hours 0-5 → 0, hours 6-15 → 1, hours 16-20 → 2, hours 21-23 → 1
// Weekend schedule: all hours → 1
pub fn fixture_rate_json() -> String {
    let weekday_row = "[0,0,0,0,0,0,1,1,1,1,1,1,1,1,1,1,2,2,2,2,2,1,1,1]";
    let weekend_row = "[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]";
    let months: Vec<&str> = vec![weekday_row; 12];
    let weekend_months: Vec<&str> = vec![weekend_row; 12];
    format!(
        r#"{{"energyweekdayschedule":[{w}],"energyweekendschedule":[{e}],"energyratestructure":[[{{"rate":0.15,"unit":"kWh"}}],[{{"rate":0.25,"unit":"kWh"}}],[{{"rate":0.40,"unit":"kWh"}}]]}}"#,
        w = months.join(","),
        e = weekend_months.join(","),
    )
}
