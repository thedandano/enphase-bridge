use enphase_bridge::inverter::snapshot::{InverterReport, parse_snapshots};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[test]
fn test_recent_inverter_is_online_with_correct_watts() {
    let reports = vec![InverterReport {
        serial_number: "SN001".to_string(),
        last_report_date: now_ts() - 60, // 1 minute ago
        last_report_watts: 250,
    }];
    let snapshots = parse_snapshots(reports, 1704067200);
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots[0].is_online);
    assert!((snapshots[0].watts_output - 250.0).abs() < 1e-6);
    assert_eq!(snapshots[0].serial_number, "SN001");
    assert_eq!(snapshots[0].window_start, 1704067200);
}

#[test]
fn test_stale_inverter_is_offline_with_zero_watts() {
    let reports = vec![InverterReport {
        serial_number: "SN002".to_string(),
        last_report_date: now_ts() - 25 * 60, // 25 minutes ago — beyond threshold
        last_report_watts: 200,
    }];
    let snapshots = parse_snapshots(reports, 1704067200);
    assert!(!snapshots[0].is_online);
    assert!((snapshots[0].watts_output - 0.0).abs() < 1e-6);
}

#[test]
fn test_multiple_inverters_correct_ordering() {
    let reports = vec![
        InverterReport {
            serial_number: "A".to_string(),
            last_report_date: now_ts() - 30,
            last_report_watts: 0,
        },
        InverterReport {
            serial_number: "B".to_string(),
            last_report_date: now_ts() - 30,
            last_report_watts: 299,
        },
    ];
    let snapshots = parse_snapshots(reports, 1704067200);
    assert_eq!(snapshots.len(), 2);
    assert!((snapshots[0].watts_output - 0.0).abs() < 1e-6);
    assert!((snapshots[1].watts_output - 299.0).abs() < 1e-6);
}

#[test]
fn test_id_field_is_zero_before_insert() {
    let reports = vec![InverterReport {
        serial_number: "X".to_string(),
        last_report_date: now_ts() - 60,
        last_report_watts: 100,
    }];
    let snapshots = parse_snapshots(reports, 0);
    assert_eq!(snapshots[0].id, 0);
}
