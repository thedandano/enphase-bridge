use enphase_bridge::collector::gateway_client::parse_session_cookie;

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
