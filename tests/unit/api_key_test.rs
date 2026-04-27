use enphase_bridge::api::middleware::api_key::{generate_api_key, is_non_loopback, resolve_api_key, validate_key};

// ── T006: Middleware passthrough when auth is disabled ─────────────────────
// When api_key = None, no validation should occur — the router passes requests
// through. The pure-function equivalent: resolve_api_key(None) returns Ok(None).

#[test]
fn test_middleware_passthrough_when_disabled() {
    // None config → no key resolved → middleware will passthrough
    let result = resolve_api_key(None);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None);
}

// ── T012: Key validation at startup ───────────────────────────────────────

#[test]
fn test_short_key_rejected() {
    let short = Some("tooshort".to_string()); // 8 chars < 32
    let result = resolve_api_key(short);
    assert!(result.is_err(), "key shorter than 32 chars must be rejected");
}

#[test]
fn test_empty_key_treated_as_none() {
    let result = resolve_api_key(Some(String::new()));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None, "empty key must be treated as absent");
}

#[test]
fn test_whitespace_key_treated_as_none() {
    let result = resolve_api_key(Some("   \t\n".to_string()));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), None, "whitespace-only key must be treated as absent");
}

#[test]
fn test_valid_key_accepted() {
    let key = "a".repeat(32);
    let result = resolve_api_key(Some(key.clone()));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some(key));
}

#[test]
fn test_key_at_exactly_32_chars_accepted() {
    let key = "12345678901234567890123456789012".to_string(); // exactly 32
    let result = resolve_api_key(Some(key.clone()));
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some(key));
}

#[test]
fn test_key_at_31_chars_rejected() {
    let key = "1234567890123456789012345678901".to_string(); // 31 chars
    let result = resolve_api_key(Some(key));
    assert!(result.is_err(), "31-char key must be rejected (< 32)");
}

// ── T013: Constant-time comparison ────────────────────────────────────────

#[test]
fn test_correct_key_passes_ct_eq() {
    let key = "my-super-secret-key-that-is-long-enough";
    assert!(validate_key(key, key), "identical keys must compare equal");
}

#[test]
fn test_wrong_key_fails_ct_eq() {
    let stored = "my-super-secret-key-that-is-long-enough";
    let supplied = "completely-wrong-key-that-is-still-long";
    assert!(!validate_key(supplied, stored), "different keys must not compare equal");
}

#[test]
fn test_partial_match_fails_ct_eq() {
    let stored = "my-super-secret-key-that-is-long-enough";
    let supplied = "my-super-secret-key-that-is-long-enoug"; // one char short
    assert!(!validate_key(supplied, stored), "partial match must not compare equal");
}

#[test]
fn test_empty_vs_stored_fails_ct_eq() {
    let stored = "my-super-secret-key-that-is-long-enough";
    assert!(!validate_key("", stored), "empty supplied key must not match stored key");
}

// ── T019: Key generation ───────────────────────────────────────────────────

#[test]
fn test_generate_api_key_returns_43_chars() {
    let key = generate_api_key();
    assert_eq!(key.len(), 43, "generated key must be exactly 43 chars (32 bytes base64url)");
}

#[test]
fn test_generate_api_key_is_base64url_alphabet() {
    let key = generate_api_key();
    let valid = key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    assert!(valid, "generated key must use only base64url chars (A-Z a-z 0-9 - _)");
}

#[test]
fn test_two_generated_keys_are_not_equal() {
    let k1 = generate_api_key();
    let k2 = generate_api_key();
    assert_ne!(k1, k2, "two generated keys must differ (basic randomness assertion)");
}

// ── TLS warning helper ─────────────────────────────────────────────────────

#[test]
fn test_loopback_ipv4_is_not_non_loopback() {
    assert!(!is_non_loopback("127.0.0.1"));
}

#[test]
fn test_loopback_ipv6_is_not_non_loopback() {
    assert!(!is_non_loopback("::1"));
}

#[test]
fn test_localhost_is_not_non_loopback() {
    assert!(!is_non_loopback("localhost"));
}

#[test]
fn test_all_interfaces_is_non_loopback() {
    assert!(is_non_loopback("0.0.0.0"), "0.0.0.0 is a non-loopback address");
}

#[test]
fn test_lan_ip_is_non_loopback() {
    assert!(is_non_loopback("192.168.1.100"), "LAN IP is non-loopback");
}

#[test]
fn test_loopback_127_0_0_2_is_not_non_loopback() {
    // The full 127.0.0.0/8 range is loopback per RFC 5735.
    // IpAddr::is_loopback() covers this; the old allowlist approach missed it.
    assert!(!is_non_loopback("127.0.0.2"), "127.0.0.2 is in the loopback range");
}

#[test]
fn test_unknown_hostname_is_non_loopback() {
    // Hostnames that don't parse as IP are treated conservatively as non-loopback.
    assert!(is_non_loopback("envoy.local"), "unknown hostname treated as non-loopback");
}
