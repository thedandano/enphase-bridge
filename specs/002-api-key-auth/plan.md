# Implementation Plan: Optional API Key Authentication

**Branch**: `002-api-key-auth` | **Date**: 2026-04-26 | **Spec**: `specs/002-api-key-auth/spec.md`

## Summary

Add optional API key authentication to the existing Axum HTTP API. Auth is disabled by default (`require_auth = false`) — zero behaviour change for existing deployments. When enabled, a router-scoped middleware layer validates `Authorization: Bearer <key>` on all `/api/*` routes except `/api/health`. The key is either user-supplied (≥32 chars, validated at startup) or auto-generated via CSPRNG (32 random bytes, base64url-encoded, ≥256 bits entropy). Invalid config causes a startup failure with a clear `ERROR` log and a distinct exit code. The feature adds no database migrations and no persistent state.

## Technical Context

**Language/Version**: Rust 1.87, edition 2024
**Framework**: Axum 0.8 (existing)
**Config**: Figment 0.10 (existing) — `[api]` section extended with `require_auth` + `api_key`
**New dependencies**:
- `subtle = "2.6"` — constant-time Bearer token comparison (FR-004)
- `zeroize = { version = "1.8", features = ["derive"] }` — key zeroization on shutdown (FR-013)
- `rand = "0.9"` — CSPRNG key generation via `OsRng` (FR-005)
- `base64 = "0.22"` — already present; base64url encoding for generated key

**Storage**: N/A — no DB entities; key lives in memory only
**Testing**: `cargo test`; unit tests in `tests/unit/`, integration tests in `tests/integration/`
**Target Platform**: Linux arm64/amd64 (Docker), home server hardware
**Performance**: Auth validation is constant-time and bounded — negligible overhead at home-server scale
**Constraints**: No DB migrations; no runtime config reload for key; auth off by default

## Constitution Check

### Gate 1 (Pre-Research)

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Rust Implementation | ✅ PASS | All new code in Rust; new deps are Rust crates |
| II. Test-First Engineering | ✅ PASS | Plan mandates failing tests written before implementation |
| III. No Silent Failures | ✅ PASS | FR-004: ERROR + exit on bad config; FR-006: WARN on auto-gen key |
| IV. Observable Operations | ✅ PASS | FR-006 (`API_KEY_GENERATED`), FR-011 (confirmation), FR-012 (TLS WARN) |
| V. Incremental Delivery | ✅ PASS | Independent vertical slice; no coupling to polling or storage |

**No violations. No complexity justification required.**

### Gate 2 (Post-Design)

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Rust Implementation | ✅ PASS | `subtle`, `zeroize`, `rand` all pure Rust |
| II. Test-First Engineering | ✅ PASS | Unit + integration tests defined in contracts before implementation tasks |
| III. No Silent Failures | ✅ PASS | Missing/wrong key → 401 with body; short key → ERROR + exit; no silent pass-through |
| IV. Observable Operations | ✅ PASS | All startup paths have distinct log events; every 401 is logged |
| V. Incremental Delivery | ✅ PASS | Router restructure is backward-compatible; feature is toggled off until user opts in |

## Project Structure

### Documentation (this feature)

```text
specs/002-api-key-auth/
├── plan.md              # This file
├── research.md          # Phase 0 output ✅
├── data-model.md        # Phase 1 output ✅
├── quickstart.md        # Phase 1 output ✅
├── contracts/
│   └── api.md           # Phase 1 output ✅
└── tasks.md             # Phase 2 output (from /speckit.tasks)
```

### Source Code

```text
Cargo.toml                        # MODIFY: add subtle, zeroize, rand

src/
├── api/
│   ├── middleware/
│   │   ├── mod.rs                # NEW: pub mod api_key;
│   │   └── api_key.rs            # NEW: api_key_middleware function
│   ├── handlers/
│   │   └── (no changes)
│   ├── mod.rs                    # MODIFY: pub mod middleware;
│   └── server.rs                 # MODIFY: AppState + router restructure
├── config.rs                     # MODIFY: require_auth + api_key on ApiConfig
└── main.rs                       # MODIFY: startup validation, key gen, logging, zeroize

tests/
├── unit/
│   └── api_key_test.rs           # NEW: key validation, generation, comparison
└── integration/
    └── api_auth_test.rs          # NEW: all acceptance scenarios (US1–US3)
```

### Router Restructure

The existing router registers `/api/health` alongside all other routes in the same `Router`, making it impossible to apply middleware selectively. The restructured router splits protected and exempt routes:

```
Router (outer, no middleware)
├── GET /api/health → health::get_health            ← EXEMPT (backward-compatible path)
└── nest /api → protected_router.route_layer(auth)
    ├── GET  /energy/windows
    ├── GET  /energy/windows/latest
    ├── GET  /inverters/snapshots
    ├── GET  /inverters/snapshots/window/{id}
    ├── GET  /inverters/arrays
    ├── POST /tou/refresh
    └── GET  /trueup/estimate
```

All existing `/api/*` paths remain unchanged. `/api/health` stays at `/api/health` (not moved to `/health`) for backward compatibility.

### Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Middleware scope | `.route_layer()` on inner router | Missing routes → 404 not 401; per FR-003 |
| State access in middleware | `from_fn_with_state(state, fn)` | Access `AppState.api_key` without global state |
| Key storage in AppState | `Option<String>` | `Clone`-compatible; `Zeroizing<String>` held in `main` for cleanup |
| Health endpoint path | Keep `/api/health` | Backward-compatible; spec assumption corrected |
| 401 response body | Identical for all failure modes | No oracle leakage per FR-010 |

### `ApiConfig` Changes

```rust
// src/config.rs — ApiConfig additions
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]               // false when absent from config.toml
    pub require_auth: bool,
    pub api_key: Option<String>,    // None = auto-generate when require_auth = true
}
```

### Startup Flow (main.rs)

```
load config
  │
  ├─ require_auth = false → skip all auth setup → run as before
  │
  └─ require_auth = true
       │
       ├─ api_key = Some(key) and key.trim().is_empty() → treat as None (auto-gen)
       │
       ├─ api_key = Some(key) and key.len() < 32
       │    → tracing::error!(event = "config_error", ...)
       │    → std::process::exit(2)  ← distinct from token-error exit(1)
       │
       ├─ api_key = Some(key) and key.len() >= 32
       │    → tracing::info!(event = "api_auth_enabled", key_len = key.len())
       │    → Zeroizing<String> holds key; clone passed to AppState
       │
       └─ api_key = None (or empty/whitespace)
            → generate_api_key() via OsRng → 43-char base64url string
            → tracing::warn!(event = "API_KEY_GENERATED", api_key = %key)
            → tracing::warn!(event = "api_key_ephemeral", message = "...")
            → Zeroizing<String> holds key; clone passed to AppState
            → if api.host is not loopback:
                 tracing::warn!(event = "api_tls_warning", message = "...")
```

### `AppState` Changes

```rust
pub struct AppState {
    pub pool: SqlitePool,
    pub token_expires_at: i64,
    pub started_at: i64,
    pub arrays: HashMap<String, Vec<String>>,
    pub tou_api_key: String,
    pub tou_rate_label: String,
    pub api_key: Option<String>,    // NEW: None = auth disabled
}
```

### Middleware Logic (`src/api/middleware/api_key.rs`)

```
receive request
  │
  ├─ state.api_key = None → pass through (auth disabled)
  │
  └─ state.api_key = Some(stored)
       │
       ├─ Authorization header missing → 401 {"error":"authentication required",...}
       ├─ Authorization header malformed (not "Bearer <token>") → 401 (same body)
       └─ token extracted
            ├─ subtle::ct_eq(supplied, stored) = false → 401 (same body)
            └─ ct_eq = true → next.run(request).await
```

## Parallel Execution Strategy

This feature has a clean parallel split for implementation. Per user instruction, tasks are designed for subagent execution.

### Phase: Tests First (TDD — two subagents in parallel)

| Subagent | Scope | Files |
|----------|-------|-------|
| **Test-A** | Unit tests — key validation, generation, comparison | `tests/unit/api_key_test.rs` |
| **Test-B** | Integration tests — all US1/US2/US3 acceptance scenarios | `tests/integration/api_auth_test.rs` |

Tests must compile and fail before implementation begins.

### Phase: Implementation (two subagents, sequential A then B)

| Subagent | Scope | Files | Dependency |
|----------|-------|-------|------------|
| **Impl-A** | Config + startup logic | `Cargo.toml`, `src/config.rs`, `src/main.rs` | None |
| **Impl-B** | Middleware + router | `src/api/middleware/`, `src/api/mod.rs`, `src/api/server.rs` | Impl-A (AppState shape) |

**Impl-A and Test-A/Test-B can run in parallel with each other** (Impl-A changes config and main; tests only touch test files). **Impl-B must follow Impl-A** (depends on updated AppState).

### Execution Order

```
[Parallel]  Test-A (unit tests)
            Test-B (integration tests)
            Impl-A (config + startup)
     ↓
[Sequential] Impl-B (middleware + router)
     ↓
[Verify]    cargo test — all tests pass
            cargo clippy -- -D warnings
```
