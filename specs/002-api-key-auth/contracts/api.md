# API Contract: Authentication Middleware

**Feature**: 002-api-key-auth | **Date**: 2026-04-26

## Scope

This contract covers the authentication middleware layer applied to all `/api/*` routes (except `/api/health`) when `require_auth = true`.

---

## Protected Routes

All routes under `/api/*` except `/api/health` require a valid API key when auth is enabled.

| Route | Method | Auth Required |
|-------|--------|---------------|
| `/api/health` | GET | Never |
| `/api/energy/windows` | GET | When enabled |
| `/api/energy/windows/latest` | GET | When enabled |
| `/api/inverters/snapshots` | GET | When enabled |
| `/api/inverters/snapshots/window/{window_start}` | GET | When enabled |
| `/api/inverters/arrays` | GET | When enabled |
| `/api/tou/refresh` | POST | When enabled |
| `/api/trueup/estimate` | GET | When enabled |

---

## Authentication Scheme

**Header**: `Authorization: Bearer <api-key>`

The API key is the 43-character base64url string shown at startup (auto-generated) or the value set in `config.toml`.

---

## Success Response

When auth is disabled or the correct API key is supplied, routes respond with their normal payloads and status codes. No auth-specific fields are added to successful responses.

---

## Failure Response: 401 Unauthorized

Returned for all auth failures: missing header, wrong key, malformed header.

**Status**: `401 Unauthorized`

**Body** (identical for all failure modes — no oracle leakage):
```json
{
  "error": "authentication required",
  "hint": "set Authorization: Bearer <api-key> header"
}
```

**Headers**:
```
Content-Type: application/json
```

**Trigger conditions** (all produce identical response):
- `Authorization` header absent
- `Authorization` header present but not in `Bearer <token>` format
- Supplied token does not match stored key (constant-time comparison)

---

## Exempt Route: /api/health

`GET /api/health` is always accessible without an API key, regardless of `require_auth` setting.

**Response** (existing, unchanged):
```json
{
  "status": "ok",
  "uptime_secs": 12345
}
```

The health response MUST NOT include version information, configuration values, gateway connectivity state, or internal metrics.

---

## Startup Signals

### Auth disabled (default)

No auth-related log events on startup.

### Auth enabled, user-supplied key

```json
{"event":"api_auth_enabled","key_len":42,"level":"INFO"}
```

### Auth enabled, auto-generated key

Two consecutive WARN log lines:

```json
{"event":"API_KEY_GENERATED","api_key":"<43-char-base64url-key>","level":"WARN"}
{"event":"api_key_ephemeral","message":"This key changes on every restart. Set api_key in config.toml to pin a stable key.","level":"WARN"}
```

### Auth enabled, non-loopback bind address

```json
{"event":"api_tls_warning","message":"API keys require transport encryption. This process does not terminate TLS — use a reverse proxy (e.g. Caddy).","level":"WARN"}
```

### Invalid key length (startup failure)

```json
{"event":"config_error","reason":"API key must be at least 32 characters; set a longer key or remove api_key from config to use auto-generation.","level":"ERROR"}
```

Process exits with code `2` (distinct from token-error exit code `1`).

---

## Key Validation Rules

| Condition | Behaviour |
|-----------|-----------|
| `require_auth = false` | All routes open; `api_key` config ignored |
| `require_auth = true`, key absent | Auto-generate 43-char base64url key at startup |
| `require_auth = true`, key is empty string | Treated as absent; auto-generate |
| `require_auth = true`, key is whitespace-only | Treated as absent; auto-generate |
| `require_auth = true`, key length < 32 | ERROR log + exit code 2 |
| `require_auth = true`, key length ≥ 32 | Use key; emit INFO confirmation |

---

## Security Properties

- **Constant-time comparison**: Token validation uses `subtle::ConstantTimeEq` — response time does not vary based on where comparison fails.
- **No oracle leakage**: All 401 responses have identical bodies — callers cannot distinguish missing header from wrong key.
- **Key never re-logged**: The API key appears in logs only once (the `API_KEY_GENERATED` event). Subsequent requests do not echo the key.
- **Memory zeroization**: The in-memory key is overwritten before process deallocation on shutdown.
