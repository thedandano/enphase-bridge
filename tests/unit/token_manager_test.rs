use enphase_bridge::auth::token_manager::TokenManager;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Construct a minimal (unsigned, HS256-labelled) JWT for testing.
/// The TokenManager only reads the `exp` field from the payload — it does
/// not verify the signature when used with local Enphase tokens.
fn make_test_token(exp: i64) -> String {
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde::Serialize;

    #[derive(Serialize)]
    struct Claims {
        iat: i64,
        exp: i64,
    }

    encode(
        &Header::default(),
        &Claims {
            iat: unix_now(),
            exp,
        },
        &EncodingKey::from_secret(b"test-secret"),
    )
    .expect("failed to encode test JWT")
}

#[test]
fn test_valid_token_not_expired() {
    let exp = unix_now() + 365 * 24 * 3600;
    let tm = TokenManager::new(make_test_token(exp));
    assert!(!tm.is_expired());
    assert!(!tm.is_near_expiry(Duration::from_secs(30 * 24 * 3600)));
}

#[test]
fn test_near_expiry_token() {
    let exp = unix_now() + 3600; // expires in 1 hour
    let tm = TokenManager::new(make_test_token(exp));
    assert!(!tm.is_expired());
    assert!(tm.is_near_expiry(Duration::from_secs(30 * 24 * 3600)));
}

#[test]
fn test_expired_token() {
    let exp = unix_now() - 60; // expired 1 minute ago
    let tm = TokenManager::new(make_test_token(exp));
    assert!(tm.is_expired());
}

#[test]
fn test_expiry_timestamp_matches_exp_claim() {
    let exp = unix_now() + 10_000;
    let tm = TokenManager::new(make_test_token(exp));
    assert_eq!(tm.expiry_timestamp(), exp);
}
