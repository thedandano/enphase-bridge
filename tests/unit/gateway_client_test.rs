use enphase_bridge::collector::gateway_client::{
    extract_cumulatives_from_json, parse_session_cookie,
};

#[test]
fn test_parse_session_cookie_extracts_session_id() {
    let header = "sessionId=EhvGBFL63CiBBB0GuASOHTMBQPqRCDDk; Secure; HttpOnly; path=/";
    assert_eq!(
        parse_session_cookie(header),
        Some("EhvGBFL63CiBBB0GuASOHTMBQPqRCDDk".to_string())
    );
}

#[test]
fn test_parse_session_cookie_no_attributes() {
    let header = "sessionId=abc123";
    assert_eq!(parse_session_cookie(header), Some("abc123".to_string()));
}

#[test]
fn test_parse_session_cookie_wrong_name_returns_none() {
    let header = "token=something; path=/";
    assert_eq!(parse_session_cookie(header), None);
}

#[test]
fn test_parse_session_cookie_empty_value() {
    let header = "sessionId=; path=/";
    assert_eq!(parse_session_cookie(header), Some(String::new()));
}

// JSON for a gateway response that includes channels arrays in both meters.
const JSON_WITH_CHANNELS: &str = r#"[
  {"eid": 704643328, "activePower": 1234.5, "actEnergyDlvd": 9876543.2, "actEnergyRcvd": 0.0, "channels": [
    {"eid": 1778385169, "activePower": 617.25, "actEnergyDlvd": 4938271.6, "actEnergyRcvd": 0.0},
    {"eid": 1778385170, "activePower": 617.25, "actEnergyDlvd": 4938271.6, "actEnergyRcvd": 0.0}
  ]},
  {"eid": 704643584, "activePower": -500.0, "actEnergyDlvd": 111111.0, "actEnergyRcvd": 22222.0, "channels": [
    {"eid": 1778385171, "activePower": -250.0, "actEnergyDlvd": 55555.5, "actEnergyRcvd": 11111.0},
    {"eid": 1778385172, "activePower": -250.0, "actEnergyDlvd": 55555.5, "actEnergyRcvd": 11111.0}
  ]}
]"#;

// JSON for a gateway response where neither meter has a channels field.
const JSON_WITHOUT_CHANNELS: &str = r#"[
  {"eid": 704643328, "activePower": 100.0, "actEnergyDlvd": 1000.0, "actEnergyRcvd": 0.0},
  {"eid": 704643584, "activePower": -50.0, "actEnergyDlvd": 500.0, "actEnergyRcvd": 0.0}
]"#;

/// 7.1a — JSON with channels arrays: channel_readings contains all 4 entries with correct fields.
#[test]
fn test_extract_cumulatives_channel_readings_populated() {
    let readings =
        extract_cumulatives_from_json(JSON_WITH_CHANNELS).expect("should parse successfully");

    assert_eq!(
        readings.channel_readings.len(),
        4,
        "expected 4 channel readings (2 per meter)"
    );

    let first = &readings.channel_readings[0];
    assert_eq!(first.meter_eid, 704643328, "first entry meter_eid");
    assert_eq!(first.channel_eid, 1778385169, "first entry channel_eid");
    assert!(
        (first.active_power - 617.25).abs() < 1e-6,
        "first entry active_power: expected 617.25, got {}",
        first.active_power
    );
    assert!(
        (first.act_energy_dlvd - 4938271.6).abs() < 1e-3,
        "first entry act_energy_dlvd mismatch"
    );
    assert!(
        (first.act_energy_rcvd - 0.0).abs() < 1e-6,
        "first entry act_energy_rcvd mismatch"
    );

    // Second meter's channels follow the first meter's.
    let third = &readings.channel_readings[2];
    assert_eq!(
        third.meter_eid, 704643584,
        "third entry meter_eid (second meter)"
    );
    assert_eq!(third.channel_eid, 1778385171, "third entry channel_eid");
}

/// 7.1b — JSON with no channels field in any meter: channel_readings is empty.
#[test]
fn test_extract_cumulatives_no_channels_returns_empty_channel_readings() {
    let readings =
        extract_cumulatives_from_json(JSON_WITHOUT_CHANNELS).expect("should parse successfully");

    assert!(
        readings.channel_readings.is_empty(),
        "channel_readings must be empty when no meter has a channels field"
    );
}
