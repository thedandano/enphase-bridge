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

// --- power-channel derivation ------------------------------------------------
//
// `activePower` on the net-consumption meter (EID 704643584) is SIGNED grid flow:
// positive = importing, negative = exporting. House load is derived as
// production + grid, because this gateway has no load CT. These fixtures use real
// values captured from live hardware where noted.

// The real 2026-08-29 sample that exposed the bug: importing 543.45 W while
// producing 2850.349 W. EID 1023410688 is present but dead (all fields zero) —
// exactly as the gateway returns it, which is why preferring it pinned grid_w to 0.
const JSON_IMPORTING_WITH_DEAD_NET_METER: &str = r#"[
  {"eid": 704643328, "activePower": 2850.349, "actEnergyDlvd": 9876543.2, "actEnergyRcvd": 0.0},
  {"eid": 704643584, "activePower": 543.45, "actEnergyDlvd": 111111.0, "actEnergyRcvd": 22222.0},
  {"eid": 1023410688, "activePower": 0.0, "actEnergyDlvd": 0.0, "actEnergyRcvd": 0.0}
]"#;

/// Importing: grid flow is positive, so house load exceeds production.
#[test]
fn test_import_positive_active_power_yields_load_above_production() {
    let readings = extract_cumulatives_from_json(JSON_IMPORTING_WITH_DEAD_NET_METER)
        .expect("should parse successfully");

    assert!(
        (readings.grid_w_now - 543.45).abs() < 1e-6,
        "grid_w_now should be the signed net-consumption activePower, got {}",
        readings.grid_w_now
    );
    assert!(
        (readings.consumption_w_now - 3393.799).abs() < 1e-6,
        "consumption_w_now should be production + grid = 3393.799, got {}",
        readings.consumption_w_now
    );
    assert!(
        readings.consumption_w_now > readings.production_w_now,
        "while importing, load must exceed production"
    );
}

/// The regression that matters most: a dead-but-present EID 1023410688 must never
/// pin grid_w to zero. This is the exact shape live hardware returns.
#[test]
fn test_dead_net_meter_does_not_zero_grid_w() {
    let readings = extract_cumulatives_from_json(JSON_IMPORTING_WITH_DEAD_NET_METER)
        .expect("should parse successfully");

    assert!(
        readings.grid_w_now != 0.0,
        "grid_w_now must come from the net-consumption meter, not the dead EID 1023410688"
    );
}

/// Exporting: grid flow is negative, so house load is below production.
#[test]
fn test_export_negative_active_power_yields_load_below_production() {
    const JSON: &str = r#"[
      {"eid": 704643328, "activePower": 3000.0, "actEnergyDlvd": 1000.0, "actEnergyRcvd": 0.0},
      {"eid": 704643584, "activePower": -1200.0, "actEnergyDlvd": 500.0, "actEnergyRcvd": 900.0}
    ]"#;
    let readings = extract_cumulatives_from_json(JSON).expect("should parse successfully");

    assert!(
        (readings.grid_w_now - -1200.0).abs() < 1e-6,
        "grid_w_now should be -1200.0 (exporting), got {}",
        readings.grid_w_now
    );
    assert!(
        (readings.consumption_w_now - 1800.0).abs() < 1e-6,
        "consumption_w_now should be 3000 - 1200 = 1800.0, got {}",
        readings.consumption_w_now
    );
    assert!(
        readings.consumption_w_now > 0.0,
        "a real house load must stay positive while exporting — this is the case the \
         old negation inverted"
    );
}

/// Night: production is zero, so the whole load is grid import. Verbatim values
/// from a real overnight snapshot.
#[test]
fn test_night_zero_production_load_equals_grid_import() {
    const JSON: &str = r#"[
      {"eid": 704643328, "activePower": 0.0, "actEnergyDlvd": 9876543.2, "actEnergyRcvd": 0.0},
      {"eid": 704643584, "activePower": 1848.818, "actEnergyDlvd": 111111.0, "actEnergyRcvd": 0.0}
    ]"#;
    let readings = extract_cumulatives_from_json(JSON).expect("should parse successfully");

    assert!(
        (readings.consumption_w_now - 1848.818).abs() < 1e-6,
        "at night the entire load is imported, got {}",
        readings.consumption_w_now
    );
    assert!(
        (readings.consumption_w_now - readings.grid_w_now).abs() < 1e-9,
        "with zero production, load and grid import must be equal"
    );
}

/// The invariant the whole derivation rests on, across every fixture.
#[test]
fn test_power_balance_closes_for_all_fixtures() {
    for (name, json) in [
        ("with_channels", JSON_WITH_CHANNELS),
        ("without_channels", JSON_WITHOUT_CHANNELS),
        ("importing", JSON_IMPORTING_WITH_DEAD_NET_METER),
    ] {
        let r = extract_cumulatives_from_json(json).expect("should parse successfully");
        assert!(
            (r.consumption_w_now - r.production_w_now - r.grid_w_now).abs() < 1e-9,
            "power balance must close for fixture {name}: {} - {} - {} != 0",
            r.consumption_w_now,
            r.production_w_now,
            r.grid_w_now
        );
    }
}

/// The existing fixtures' meanings, restated under the corrected convention:
/// a negative activePower means exporting, so load sits below production.
#[test]
fn test_existing_fixtures_read_as_exporting() {
    let with_channels =
        extract_cumulatives_from_json(JSON_WITH_CHANNELS).expect("should parse successfully");
    assert!(
        (with_channels.grid_w_now - -500.0).abs() < 1e-6,
        "JSON_WITH_CHANNELS exports 500 W"
    );
    assert!(
        (with_channels.consumption_w_now - 734.5).abs() < 1e-6,
        "1234.5 produced - 500 exported = 734.5 load, got {}",
        with_channels.consumption_w_now
    );

    let without_channels =
        extract_cumulatives_from_json(JSON_WITHOUT_CHANNELS).expect("should parse successfully");
    assert!(
        (without_channels.consumption_w_now - 50.0).abs() < 1e-6,
        "100 produced - 50 exported = 50 load, got {}",
        without_channels.consumption_w_now
    );
}

/// Both meters are required: absence must error, never silently read as zero watts.
#[test]
fn test_missing_meter_is_an_error_not_a_silent_zero() {
    const PRODUCTION_ONLY: &str = r#"[
      {"eid": 704643328, "activePower": 1000.0, "actEnergyDlvd": 1000.0, "actEnergyRcvd": 0.0}
    ]"#;
    const NET_CONSUMPTION_ONLY: &str = r#"[
      {"eid": 704643584, "activePower": 500.0, "actEnergyDlvd": 500.0, "actEnergyRcvd": 0.0}
    ]"#;

    assert!(
        extract_cumulatives_from_json(PRODUCTION_ONLY).is_err(),
        "a missing net-consumption meter must be an error"
    );
    assert!(
        extract_cumulatives_from_json(NET_CONSUMPTION_ONLY).is_err(),
        "a missing production meter must be an error, not production_w_now = 0.0"
    );
}
