use std::collections::HashSet;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use enphase_bridge::api::middleware::api_key::generate_api_key;
use enphase_bridge::api::server::{AppState, create_router};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tower::ServiceExt;

async fn test_pool() -> SqlitePool {
    let opts = SqliteConnectOptions::new()
        .filename(":memory:")
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("in-memory pool");
    sqlx::migrate!("./migrations").run(&pool).await.expect("migrations");
    pool
}

fn make_state(pool: SqlitePool, api_key: Option<String>) -> AppState {
    AppState {
        pool,
        token_expires_at: 9_999_999_999,
        started_at: 0,
        arrays: Default::default(),
        tou_api_key: String::new(),
        tou_rate_label: String::new(),
        api_key: api_key.map(|k| Arc::from(k.as_str())),
    }
}

async fn body_bytes(resp: axum::http::Response<Body>) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap().to_vec()
}

async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    serde_json::from_slice(&body_bytes(resp).await).unwrap()
}

// ── T007: Default auth disabled — existing routes work with no header ──────

#[tokio::test]
async fn test_default_auth_disabled() {
    let app = create_router(make_state(test_pool().await, None));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "default config (no auth) must allow unauthenticated access"
    );
}

// ── T008: Health endpoint always accessible ────────────────────────────────

#[tokio::test]
async fn test_health_always_accessible_no_auth_configured() {
    let app = create_router(make_state(test_pool().await, None));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "/api/health must return 200 when auth is disabled");
}

#[tokio::test]
async fn test_health_always_accessible_with_auth_enabled() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key)));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "/api/health must be exempt from auth");
}

// ── T014: Auth enforcement — 401 for all failure modes ────────────────────

#[tokio::test]
async fn test_missing_header_returns_401() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key)));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "missing header must return 401");
}

#[tokio::test]
async fn test_wrong_key_returns_401() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key)));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", "Bearer wrongkey")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "wrong key must return 401");
}

#[tokio::test]
async fn test_malformed_header_returns_401() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key)));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", "NotBearer abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "malformed header must return 401");
}

#[tokio::test]
async fn test_empty_bearer_token_returns_401() {
    // "Bearer " (with trailing space) — strip_prefix yields Some("") which fails validate_key.
    // Pinned here so a refactor to split_whitespace cannot silently change semantics.
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key)));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", "Bearer ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "empty bearer token must return 401");
}

#[tokio::test]
async fn test_correct_key_returns_200() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "correct key must return 200");
}

/// SC-005: all 7 protected routes from contracts/api.md must return 401 when
/// auth is enabled and no Authorization header is provided.
#[tokio::test]
async fn test_all_protected_routes_return_401() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();

    let protected_routes = &[
        ("GET", "/api/energy/windows"),
        ("GET", "/api/energy/windows/latest"),
        ("GET", "/api/inverters/snapshots"),
        ("GET", "/api/inverters/snapshots/window/1704067200"),
        ("GET", "/api/inverters/arrays"),
        ("POST", "/api/tou/refresh"),
        ("GET", "/api/trueup/estimate"),
    ];

    for (method, uri) in protected_routes {
        let app = create_router(make_state(test_pool().await, Some(key.clone())));
        let resp = app
            .oneshot(
                Request::builder()
                    .method(*method)
                    .uri(*uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri} must return 401 when auth is enabled and no header provided"
        );
    }
}

// ── T015: Identical 401 body for all failure modes (no oracle leakage) ─────

#[tokio::test]
async fn test_401_body_identical_for_all_failure_modes() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();

    let scenarios: &[(&str, Option<&str>)] = &[
        ("/api/energy/windows", None),                         // missing header
        ("/api/energy/windows", Some("Bearer wrongkey")),      // wrong key
        ("/api/energy/windows", Some("NotBearer abc")),        // malformed scheme
        ("/api/energy/windows", Some("Bearer ")),              // empty token
    ];

    let mut bodies: Vec<Vec<u8>> = Vec::new();
    for (uri, auth_header) in scenarios {
        let app = create_router(make_state(test_pool().await, Some(key.clone())));
        let mut builder = Request::builder().uri(*uri);
        if let Some(hdr) = auth_header {
            builder = builder.header("Authorization", *hdr);
        }
        let resp = app
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        bodies.push(body_bytes(resp).await);
    }

    let first = &bodies[0];
    for (i, body) in bodies.iter().enumerate().skip(1) {
        assert_eq!(
            first, body,
            "401 body for scenario {i} differs from scenario 0 — oracle leakage risk"
        );
    }

    let body_str = String::from_utf8_lossy(first);
    assert!(
        body_str.contains("authentication required"),
        "401 body must contain the string 'authentication required'"
    );
}

// ── T016: Configured key auth state applied ───────────────────────────────

#[tokio::test]
async fn test_configured_key_auth_state_applied() {
    let key = "a-valid-43-char-base64url-key-for-testing!!".to_string();
    assert!(key.len() >= 32);

    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "configured key must authenticate successfully");
}

// ── T020: Auto-generated key authenticates + wrong copy does not ───────────

#[tokio::test]
async fn test_autogen_key_authenticates_successfully() {
    let key = generate_api_key();
    assert_eq!(key.len(), 43, "auto-generated key must be 43 chars");

    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "auto-generated key must authenticate");
}

#[tokio::test]
async fn test_autogen_key_wrong_copy_returns_401() {
    let key = generate_api_key();
    let wrong = generate_api_key();
    assert_ne!(key, wrong);

    let app = create_router(make_state(test_pool().await, Some(key)));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("Bearer {wrong}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Wiring contract: middleware contract when api_key is None ─────────────

/// The middleware passes all requests when `AppState.api_key` is `None`.
/// Enforcement of `require_auth=true` + key resolution lives in `startup.rs`.
/// This test pins the middleware contract so a change is always visible.
#[tokio::test]
async fn test_middleware_passes_all_when_api_key_is_none() {
    let app = create_router(make_state(test_pool().await, None));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "middleware with api_key=None must pass all requests — enforcement is in startup.rs"
    );
}

// ── Bearer scheme edge cases ───────────────────────────────────────────────

/// RFC 7235 allows case-insensitive scheme names, but this implementation
/// enforces exact "Bearer " prefix. This test pins that strict behavior so any
/// "be lenient" change is visible and intentional.
#[tokio::test]
async fn test_bearer_scheme_is_case_sensitive() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "'bearer' (lowercase) must return 401 — scheme matching is case-sensitive"
    );
}

/// A key pasted from a terminal with a trailing space must be rejected.
/// Pins this contract before someone adds a `.trim()` that would weaken validation.
#[tokio::test]
async fn test_trailing_whitespace_in_token_returns_401() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("Bearer {key} "))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "token with trailing whitespace must return 401 — keys must match exactly"
    );
}

// ── Key leakage checks ────────────────────────────────────────────────────

/// The stored API key must never appear in any response body — not in 401 errors,
/// not in health 200, not in authenticated data responses.
#[tokio::test]
async fn test_api_key_absent_from_response_bodies() {
    let key = generate_api_key();

    // 401 response must not echo the key
    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_bytes(resp).await;
    assert!(
        !String::from_utf8_lossy(&body).contains(&key),
        "401 response body must not contain the stored API key"
    );
}

#[tokio::test]
async fn test_200_response_does_not_leak_api_key() {
    let key = generate_api_key();

    // Health 200 (auth-exempt path) must not echo the key
    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp).await;
    assert!(
        !String::from_utf8_lossy(&body).contains(&key),
        "health 200 must not echo the api key"
    );

    // Authenticated 200 must not echo the key
    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_bytes(resp).await;
    assert!(
        !String::from_utf8_lossy(&body).contains(&key),
        "authenticated 200 response must not echo the api key"
    );
}

// ── T021: TLS warning path — auth works regardless of bind address ─────────

#[tokio::test]
async fn test_auth_works_regardless_of_bind_address() {
    let key = "a-valid-32-char-minimum-key-here!!!".to_string();
    let app = create_router(make_state(test_pool().await, Some(key.clone())));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/energy/windows")
                .header("Authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Health response shape (allowlist) ─────────────────────────────────────

/// Health response must contain exactly the documented fields — no more.
/// Allowlist approach: any new field that leaks internal state fails this test,
/// requiring an explicit decision to add it.
#[tokio::test]
async fn test_health_response_is_liveness_only() {
    let app = create_router(make_state(test_pool().await, None));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let j = json_body(resp).await;

    let allowed: HashSet<&str> =
        ["status", "last_window_start", "token_expires_at", "uptime_seconds"]
            .into_iter()
            .collect();
    let actual: HashSet<&str> = j
        .as_object()
        .expect("health response must be a JSON object")
        .keys()
        .map(String::as_str)
        .collect();

    assert_eq!(
        actual, allowed,
        "health response contains unexpected fields — check for info leakage. \
         Add new fields explicitly to the allowlist above."
    );
    assert_eq!(j["status"], "ok");
}
