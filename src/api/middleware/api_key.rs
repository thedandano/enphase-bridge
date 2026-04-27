use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use subtle::ConstantTimeEq;

/// Auth state used exclusively by this middleware — decoupled from the full AppState
/// so the middleware has no dependency on server internals.
#[derive(Clone)]
pub struct AuthState {
    pub api_key: Option<Arc<str>>,
}

/// Generates a CSPRNG 43-char base64url key (32 random bytes → 256 bits entropy).
/// `rand::rng()` returns a ThreadRng backed by ChaCha12, seeded from the OS.
pub fn generate_api_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Returns true when `supplied` equals `stored` using constant-time comparison.
///
/// Key length is not a secret — auto-generated keys are always 43 chars (documented),
/// and configured keys expose their length via the minimum-32-char requirement.
/// The explicit length check here avoids `ct_eq`'s early exit on length mismatch;
/// both branches take the same action (return false), so no timing info is leaked
/// beyond what the public key format already reveals.
pub fn validate_key(supplied: &str, stored: &str) -> bool {
    let s = supplied.as_bytes();
    let k = stored.as_bytes();
    if s.len() != k.len() {
        return false;
    }
    s.ct_eq(k).into()
}

/// Resolves the `api_key` config value:
/// - `None` / empty / whitespace → `Ok(None)` (auto-generate at startup)
/// - `< 32` chars → `Err(message)` (startup refuses)
/// - `≥ 32` chars → `Ok(Some(key))`
pub fn resolve_api_key(raw: Option<String>) -> Result<Option<String>, String> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) if s.len() < 32 => Err(
            "API key must be at least 32 characters; set a longer key or remove \
             api_key from config to use auto-generation."
                .to_owned(),
        ),
        Some(s) => Ok(Some(s)),
    }
}

/// Returns `true` when `host` is not a loopback address.
///
/// Parses the host as an `IpAddr` and uses `is_loopback()`, which correctly covers
/// the full 127.0.0.0/8 range and `::1`. The hostname "localhost" is treated as
/// loopback. Other hostnames that fail IP parsing are conservatively treated as
/// non-loopback so the TLS warning fires.
pub fn is_non_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(addr) => !addr.is_loopback(),
        Err(_) => true, // unknown hostname — conservative: assume non-loopback, emit TLS warning
    }
}

/// Router-scoped middleware — enforces Bearer token auth when `auth.api_key` is `Some`.
/// When `api_key` is `None`, all requests pass through without header inspection.
pub async fn api_key_middleware(
    State(auth): State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(ref stored_key) = auth.api_key else {
        return next.run(request).await;
    };

    let bearer = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    match bearer {
        Some(token) if validate_key(token, stored_key) => next.run(request).await,
        _ => unauthorized(),
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "authentication required",
            "hint": "set Authorization: Bearer <api-key> header"
        })),
    )
        .into_response()
}
