use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

pub struct TokenManager {
    exp: i64,
}

impl TokenManager {
    pub fn new(raw_token: String) -> Self {
        let exp = Self::parse_expiry(&raw_token).unwrap_or_else(|e| {
            warn!(event = "token_parse_warning", error = %e,
                  message = "could not parse token expiry; assuming expired");
            0
        });
        debug!(event = "token_loaded", expires_at = exp);
        Self { exp }
    }

    // Decode JWT payload without any signature or claims validation —
    // Enphase tokens use cloud-issued ES256 signatures; we only need `exp`.
    fn parse_expiry(token: &str) -> Result<i64, String> {
        let payload_b64 = token
            .split('.')
            .nth(1)
            .ok_or("malformed JWT: missing payload segment")?;

        let bytes = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|e| format!("base64 decode error: {e}"))?;

        let claims: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("JSON parse error: {e}"))?;

        claims["exp"]
            .as_i64()
            .ok_or_else(|| "missing or non-integer 'exp' claim".to_string())
    }

    pub fn expiry_timestamp(&self) -> i64 {
        self.exp
    }

    pub fn is_expired(&self) -> bool {
        let now = unix_now();
        self.exp <= now
    }

    pub fn is_near_expiry(&self, threshold: Duration) -> bool {
        let now = unix_now();
        let threshold_secs = threshold.as_secs() as i64;
        self.exp <= now + threshold_secs
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
