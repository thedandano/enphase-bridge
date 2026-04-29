## Context

The IQ Gateway exposes two auth-protected endpoint families:

- `/api/v1/` — legacy endpoints; accept the Entrez cloud JWT directly via `Authorization: Bearer`
- `/ivp/` — IVP endpoints; firmware 7.x+ now requires a local session token in addition to (or instead of) the cloud JWT

The session token is obtained by calling `POST /auth/check_jwt` with the cloud JWT. The gateway responds with a `Set-Cookie: sessionId=<token>` header. Subsequent IVP requests must include `Cookie: sessionId=<token>`.

Observed behavior:
- `POST /auth/check_jwt` → 200, `Set-Cookie: sessionId=...`
- `GET /ivp/meters/readings` with only Bearer → 401
- `GET /api/v1/production/inverters` with only Bearer → 200

The `sessionId` cookie has no explicit `Max-Age` or `Expires` in the response, so its lifetime is unknown. Re-auth on 401 is necessary to handle expiry at runtime.

## Goals / Non-Goals

**Goals:**
- Restore `/ivp/meters/readings` access on firmware 7.x+
- Handle session expiry gracefully without daemon restart
- Keep `GatewayClient`'s public API unchanged for callers

**Non-Goals:**
- Supporting multiple simultaneous sessions
- Persisting the session token across restarts (re-auth on startup is cheap)
- Validating the gateway's ES256 JWT signature (already out of scope per existing design)
- Changing auth for `/api/v1/production/inverters` (still works without session cookie)

## Decisions

### 1. Store session token in `GatewayClient` as `Option<String>`

`GatewayClient` gains a `session_id: Option<String>` field. `check_jwt()` populates it. IVP request methods attach it via `Cookie` header when present.

**Alternative considered**: Use `reqwest`'s built-in cookie jar. Rejected — the gateway's cookie has `HttpOnly; Secure` but no domain, which causes cookie jar matching to be unreliable across request builders. Explicit header attachment is simpler and deterministic.

### 2. Re-auth on 401, retry once

When an IVP endpoint returns 401, call `check_jwt()` to refresh the session and retry the request once. If it fails again, propagate the error normally.

**Alternative considered**: Pre-emptive token refresh on a timer. Rejected — token lifetime is unknown; polling for expiry wastes cycles. React-on-401 is simpler and correct.

### 3. `check_jwt()` is called by `Scheduler` before the poll loop starts

`Scheduler::run()` calls `self.gateway.check_jwt().await` once before entering the loop. This ensures the session is ready before the first poll. Re-auth on 401 handles any subsequent expiry.

**Alternative considered**: Lazy init on first IVP request. Rejected — a startup failure is cleaner to diagnose than a first-poll failure that looks like a transient error.

### 4. `GatewayClient` uses interior mutability (`tokio::sync::Mutex` or `Arc<Mutex>`) for session state

Since `GatewayClient` is shared by reference and `check_jwt()` mutates `session_id`, use a `Mutex<Option<String>>` for the session field.

**Simpler alternative**: Make `check_jwt()` take `&mut self` and store the token before creating the scheduler. This avoids `Mutex` entirely. Prefer this — `GatewayClient` is owned by `Scheduler`, not shared, so `&mut self` is sufficient. Use `Cell` or plain mutation via `&mut`.

**Decision**: Use `&mut self` on `check_jwt()` and store in a plain `Option<String>`. The scheduler owns the client exclusively.

## Risks / Trade-offs

- **Session lifetime unknown** — The gateway doesn't advertise cookie expiry. If sessions are very short-lived (< 1 min), re-auth-on-401 adds latency to one poll per expiry cycle. Acceptable given 60s poll interval.
- **Firmware version assumptions** — This adds session auth for all `/ivp/` requests. On older firmware that doesn't set a cookie, `check_jwt()` will still return 200 but `session_id` may be empty; attaching an empty cookie header is harmless.
- **One retry** — A single retry after re-auth is conservative. If the gateway is temporarily unhealthy, one retry is sufficient to fail fast without a tight retry loop.

## Migration Plan

1. Deploy updated daemon binary
2. On startup: `check_jwt()` runs, session established
3. First poll: `/ivp/meters/readings` succeeds with session cookie
4. Window boundaries resume being detected and written

No database migration required. No config changes required. Rollback: revert binary.

## Open Questions

- None — behavior is fully observed via direct curl testing.
