# Research: Optional API Key Authentication

**Feature**: 002-api-key-auth | **Date**: 2026-04-26

## Constant-Time Comparison

**Decision**: `subtle = "2.6"` — `ConstantTimeEq` trait on `&[u8]`

**Rationale**: The `subtle` crate is the de-facto standard for timing-safe comparisons in the Rust ecosystem. It returns `subtle::Choice` (not `bool`) to prevent compiler optimizations from introducing data-dependent branches. Converting via `.into()` yields a `bool` without reintroducing timing variance.

```toml
subtle = "2.6"
```

```rust
use subtle::ConstantTimeEq;

fn verify_api_key(supplied: &str, stored: &str) -> bool {
    supplied.as_bytes().ct_eq(stored.as_bytes()).into()
}
```

`ConstantTimeEq` operates on `&[u8]`; call `.as_bytes()` on `&str`. Comparison is constant-time regardless of where the first differing byte occurs.

**Alternatives considered**: `ring::constant_time::verify_slices_are_equal` — also correct, but pulls in the full `ring` dependency (already not in this project's tree). `subtle` is lighter and purpose-built.

---

## Memory Zeroization on Shutdown

**Decision**: `zeroize = { version = "1.8", features = ["derive"] }` — `ZeroizeOnDrop` derive

**Rationale**: `zeroize` provides `ZeroizeOnDrop` as a derive macro that overwrites the heap allocation before deallocation. `String` is supported out of the box. Wrapping the key in `Zeroizing<String>` in `main.rs` gives automatic cleanup when the variable goes out of scope (i.e., on process shutdown).

```toml
zeroize = { version = "1.8", features = ["derive"] }
```

```rust
use zeroize::Zeroizing;

let api_key: Zeroizing<String> = Zeroizing::new(generate_api_key());
// key is zeroized automatically when `api_key` drops (process exit)
```

**Pattern for `AppState`**: `AppState` is `Clone`; storing `Zeroizing<String>` directly creates multiple copies. Hold one `Zeroizing<String>` in `main.rs` for cleanup, and pass a plain `String` clone into `AppState`. This satisfies FR-013 (zeroized on shutdown) without complicating the `Clone` bound.

**Alternatives considered**: `secrecy` crate — higher-level but adds `Debug` redaction; overkill for this use case. `zeroize` is simpler and widely audited.

---

## CSPRNG Key Generation

**Decision**: `rand = "0.9"` with `OsRng` + `base64 0.22` (already in `Cargo.toml`) for base64url encoding

**Rationale**: `OsRng` delegates directly to the OS CSPRNG (`/dev/urandom` on Linux, `BCryptGenRandom` on Windows). It is stateless, has no seedable state to leak, and requires no extra features in a `std` binary. 32 bytes yields exactly 256 bits of entropy, encoded as 43 base64url characters (no padding).

```toml
rand = "0.9"
# base64 = "0.22" already present
```

```rust
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{RngCore, rngs::OsRng};

pub fn generate_api_key() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
```

Output: 43-character base64url string, 256 bits of OS-sourced entropy.

**Alternatives considered**: `getrandom` directly — lower level, no benefit here since `rand` wraps it. UUID v4 — only 122 bits of entropy and wrong encoding for this use case.

---

## Axum 0.8 Scoped Middleware

**Decision**: `axum::middleware::from_fn_with_state` + `.route_layer()` on the inner router, then `Router::nest("/api", inner)`

**Rationale**: Applying `.route_layer()` to the inner router before nesting ensures the middleware only covers protected routes. Using `.route_layer()` (not `.layer()`) means an unmatched route returns `404`, not a middleware-intercepted `401`. `from_fn_with_state` passes `AppState` to the middleware function, giving it access to the configured key without global state.

```rust
use axum::{middleware, Router, routing::{get, post}};

// Inner router — protected routes only (no /health)
let protected = Router::new()
    .route("/energy/windows", get(energy::get_windows))
    // ... other /api/* routes (without the /api prefix — nest adds it)
    .route_layer(middleware::from_fn_with_state(state.clone(), api_key_middleware));

// Outer router — health exempt, everything else nested
let app = Router::new()
    .route("/api/health", get(health::get_health))  // exempt — stays at /api/health
    .nest("/api", protected)
    .with_state(state);
```

The middleware function signature in Axum 0.8:
```rust
use axum::{extract::{Request, State}, middleware::Next, response::Response};

async fn api_key_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    // extract and verify Authorization header
}
```

**Health endpoint path**: The existing code uses `/api/health` (not `/health` as noted in the spec Assumptions). This plan keeps `/api/health` for backward compatibility — it is registered on the outer router, outside the middleware scope, preserving its existing path.

**Key Axum 0.8 changes vs 0.7**: `Next` is `axum::middleware::Next` with no body generic; `Request` is `axum::extract::Request`; `FromRequestParts` implementations use `async fn` directly (no `#[async_trait]`).

**Alternatives considered**: `tower::ServiceBuilder` — lower-level, more verbose, no benefit for this use case. Per-handler `FromRequestParts` extractor — rejected per FR-003 (router-scoped middleware required).
