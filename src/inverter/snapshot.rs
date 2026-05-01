use crate::storage::models::MicroinverterSnapshot;
use serde::Deserialize;

// Inverter is considered offline if last report is older than 20 minutes
const OFFLINE_THRESHOLD_SECS: i64 = 20 * 60;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InverterReport {
    pub serial_number: String,
    pub last_report_date: i64,
    pub last_report_watts: i64,
}

pub fn parse_snapshots(
    reports: Vec<InverterReport>,
    window_start: i64,
) -> Vec<MicroinverterSnapshot> {
    let now = unix_now();
    reports
        .into_iter()
        .map(|r| {
            let is_online = (now - r.last_report_date) < OFFLINE_THRESHOLD_SECS;
            MicroinverterSnapshot {
                id: 0,
                window_start,
                serial_number: r.serial_number,
                watts_output: if is_online {
                    r.last_report_watts as f64
                } else {
                    0.0
                },
                is_online,
                last_report_date: r.last_report_date,
            }
        })
        .collect()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
