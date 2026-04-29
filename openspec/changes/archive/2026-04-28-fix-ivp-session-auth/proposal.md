## Why

IQ Gateway firmware 7.x+ requires a local session token (obtained via `POST /auth/check_jwt`) for IVP endpoints such as `/ivp/meters/readings`. The daemon currently sends only the raw Entrez cloud JWT, which now returns 401 on that endpoint. This causes every meter poll to fail, halting all energy window writes and making all inverters appear offline.

## What Changes

- `GatewayClient` acquires a local session token at startup by calling `POST /auth/check_jwt` with the cloud JWT
- The session cookie (`sessionId`) is attached to all `/ivp/` requests
- `GatewayClient` re-authenticates automatically when an IVP request returns 401
- `/api/v1/production/inverters` is unaffected (still accepts the cloud JWT directly)

## Capabilities

### New Capabilities
- `gateway-session-auth`: Two-step gateway authentication — exchange cloud JWT for a local session token via `check_jwt`, attach the session cookie to IVP requests, and re-auth on 401

### Modified Capabilities
<!-- none — no existing spec-level requirements are changing -->

## Impact

- **`src/collector/gateway_client.rs`**: Primary change — add `check_jwt()`, session storage, and cookie injection
- **`src/collector/scheduler.rs`**: Trigger `check_jwt` on startup before the poll loop
- **`src/main.rs`** or **`src/startup.rs`**: May need to thread session init through the startup path
- **No API surface changes** — callers of `GatewayClient` are unaffected
- **No config changes** — no new fields needed; session token is ephemeral runtime state
