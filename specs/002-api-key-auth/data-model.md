# Data Model: Optional API Key Authentication

**Feature**: 002-api-key-auth | **Date**: 2026-04-26

## Overview

This feature introduces no new database tables or migrations. All state is in-memory for the lifetime of the process. There are two logical entities.

---

## Entity: ApiKey

The secret value used to authenticate HTTP API requests.

| Attribute | Type | Source | Constraints |
|-----------|------|--------|-------------|
| `value` | `String` | User config or CSPRNG auto-gen | ≥32 chars (user-supplied); exactly 43 chars (auto-generated base64url) |
| `origin` | Enum: `UserSupplied` \| `Generated` | Derived at startup | Determines startup log behavior |

**Lifecycle**:
- Created once at startup during `Config::load()` validation
- Held in memory (in `AppState`) for the lifetime of the process
- Zeroized when the process exits
- Never written to disk or logged after initial generation notice

**State transitions**:
```
absent (require_auth = false)
    → active-user-supplied (require_auth = true, api_key set, len ≥ 32)
    → active-generated (require_auth = true, api_key absent/empty/whitespace)
    → config-error (require_auth = true, api_key set, len < 32) → process exits
```

---

## Entity: AuthConfiguration

The subset of `ApiConfig` that drives authentication behaviour.

| Attribute | Type | Config Key | Default |
|-----------|------|------------|---------|
| `require_auth` | `bool` | `[api] require_auth` | `false` |
| `api_key` | `Option<String>` | `[api] api_key` | `None` |

**Rules**:
- `require_auth = false` → `api_key` field is ignored at runtime
- `require_auth = true`, `api_key = None` → auto-generate key at startup
- `require_auth = true`, `api_key = Some("")` or whitespace-only → treated as `None` (auto-generate)
- `require_auth = true`, `api_key = Some(s)` where `s.len() < 32` → startup failure, exit code 2
- `require_auth = true`, `api_key = Some(s)` where `s.len() >= 32` → use as-is

**Config file representation** (`config.toml`):

```toml
[api]
host = "0.0.0.0"
port = 8080
require_auth = true
api_key = "your-secret-key-at-least-32-chars"  # omit to auto-generate
```
