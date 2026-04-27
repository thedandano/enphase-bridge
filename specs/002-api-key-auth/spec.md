# Feature Specification: Optional API Key Authentication

**Feature Branch**: `002-api-key-auth`
**Created**: 2026-04-26
**Status**: Draft

## Glossary

- **API key**: The secret value a user copies from the startup log or sets in the config file. This is the canonical user-facing term throughout this spec and all documentation.
- **Bearer token**: The HTTP transport mechanism — the API key is sent as `Authorization: Bearer <api-key>` in request headers. Users interact with the API key; "Bearer token" is a protocol detail reserved for technical contexts.

## Clarifications

### Session 2026-04-26

- Q: Should the service validate the length of user-supplied API keys at startup? → A: Minimum 32 characters required; service refuses to start with a shorter key and logs a clear error.
- Q: Should the service rate-limit failed authentication attempts? → A: No built-in rate limiting; this is explicitly out of scope — reverse proxy or OS-level controls (e.g., Fail2ban, firewall rules) handle throttling.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Zero-Friction Default Access (Priority: P1)

A home user runs the service on their local network behind a TLS-terminating reverse proxy (e.g., Caddy). They want to query energy data from their own scripts, dashboard, or mobile app without any authentication ceremony. Since the service is installed with defaults, no API key is required — all API endpoints are open.

**Why this priority**: The majority of users are homeowners on a trusted LAN who should not be forced through auth setup. The North Star demands sensible defaults ship first. Requiring auth by default would create friction for the primary use case.

**Independent Test**: Run the service with default configuration, send any request to `/api/*`, confirm a `200` response with data — no `Authorization` header required.

**Acceptance Scenarios**:

1. **Given** the service is running with default configuration, **When** a client sends a `GET /api/power` request with no `Authorization` header, **Then** the service returns the requested data with a `200` status.
2. **Given** the service is running with default configuration, **When** a client sends any `/api/*` request, **Then** no authentication error is returned regardless of whether an `Authorization` header is present or absent.

---

### User Story 2 - Enable API Key Protection (Priority: P2)

A user who exposes the service beyond their home LAN (e.g., via port forwarding, VPN endpoint, or cloud hosting) wants to restrict access to only clients who hold a known API key. They enable authentication via a single configuration option. After doing so, all API routes reject requests that do not carry a valid API key in the `Authorization: Bearer` header.

**Why this priority**: Open source distribution means deployments vary widely. Some users will expose the service to the internet. A single config toggle is the minimal protection layer they need. Without it, the daemon has no access control in exposed deployments.

**Independent Test**: Enable auth in config with an explicit key value, send a request without the key (expect `401`), then send with the correct key (expect `200`).

**Acceptance Scenarios**:

1. **Given** auth is enabled and a key is configured, **When** a client sends a request to `/api/*` with no `Authorization` header, **Then** the service returns `401 Unauthorized`.
2. **Given** auth is enabled and a key is configured, **When** a client sends a request with `Authorization: Bearer <wrong-key>`, **Then** the service returns `401 Unauthorized`.
3. **Given** auth is enabled and a key is configured, **When** a client sends a request with `Authorization: Bearer <correct-key>`, **Then** the service returns the requested data with a `200` status.
4. **Given** auth is enabled, **When** a `401` is returned for any reason (missing header, wrong key, or malformed header), **Then** the response body contains the string "authentication required" and includes the instruction to use the `Authorization: Bearer <api-key>` header; the response body is identical regardless of the specific failure mode.

---

### User Story 3 - Auto-Generated Key on First Enable (Priority: P3)

A user enables authentication in the configuration but does not want to choose their own key. The service generates a secure random API key on startup, makes it clearly visible in the startup log with a machine-findable tag, and prominently warns that the key changes on every restart until the user sets a static key in config.

**Why this priority**: Requiring users to generate cryptographically secure keys themselves is friction. Auto-generation with prominent logging is the zero-friction path to enabled security — users opt in with one config line, the service does the rest.

**Independent Test**: Enable auth in config without setting a key value; start the service; confirm a log line containing the tag `API_KEY_GENERATED` and the key value appears; use that key to successfully authenticate to `/api/*`.

**Acceptance Scenarios**:

1. **Given** auth is enabled and no key is configured, **When** the service starts, **Then** a startup log line at `WARN` level contains the literal tag `API_KEY_GENERATED` and the generated key value on a single line.
2. **Given** auth is enabled and no key is configured, **When** the startup banner is emitted, **Then** it includes a second line warning that the key changes on every restart and directing the user to set `api_key` in config to pin a stable key.
3. **Given** auth is enabled and no key is configured, **When** the auto-generated key is used in the `Authorization: Bearer` header, **Then** the service returns `200` for valid API requests.
4. **Given** auth is enabled and no key is configured, **When** the service restarts, **Then** a new key is generated; clients must recopy the key from the startup log unless the user has since set an explicit key in config.
5. **Given** auth is enabled and a user-configured key loads successfully, **When** the service starts, **Then** a single `INFO` log line states "API authentication enabled — using configured key" and includes the key length in characters.

---

### Edge Cases

- What happens when the `Authorization` header is present but malformed (e.g., `Authorization: notbearer abc`)? The service returns `401` with the same "authentication required" response body as any other auth failure — identical body prevents oracle leakage about the specific failure mode.
- What happens when auth is enabled but the config key is an empty string or a whitespace-only string? Both are treated as "not configured" and the service auto-generates a key.
- What happens when auth is enabled and the configured key is fewer than 32 characters? The service refuses to start, logs "API key must be at least 32 characters; set a longer key or remove `api_key` from config to use auto-generation," and exits with exit code 2 (configuration error, distinct from the token-error exit code 1).
- What happens when a non-API route (e.g., `/health`) receives a request? Only explicitly listed exempt routes are reachable without an API key. They return only liveness state (up/down) and MUST NOT include version information, gateway details, or internal metrics.
- What happens when the service is restarted with auth disabled after previously being enabled? All requests succeed without an API key again — no residual state.
- What happens when a misconfigured key causes the service to refuse to start under Docker's `restart: unless-stopped` policy? The container enters a restart loop by design; the ERROR log emitted before exit makes the cause immediately identifiable. Users must fix the config and restart manually.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The service MUST default to no authentication required on all `/api/*` routes.
- **FR-002**: The service MUST provide a single configuration option to enable API key authentication (`require_auth = true/false` in the `[api]` config section).
- **FR-003**: When authentication is enabled, auth MUST be enforced as a router-scoped middleware layer covering all `/api/*` routes; per-handler auth checks are prohibited. The service MUST reject all unprotected requests with `401 Unauthorized`.
- **FR-004**: When authentication is enabled and a key is explicitly configured, the service MUST validate incoming API keys using a constant-time comparison primitive to prevent timing attacks. The configured key MUST be at least 32 characters; if shorter, the service MUST refuse to start, log a clear actionable error at `ERROR` level, and exit with a configuration-error exit code distinct from transient failures.
- **FR-005**: When authentication is enabled and no key is configured (including empty strings and whitespace-only values), the service MUST auto-generate a key using a CSPRNG (e.g., `OsRng`), producing at least 32 random bytes encoded as base64url (~43 characters, ≥256 bits of entropy).
- **FR-006**: When an auto-generated key is produced, the service MUST emit a `WARN` log line containing the literal tag `API_KEY_GENERATED` and the key value on a single line, followed immediately by a second line warning that the key changes on every restart and directing the user to set `api_key` in config for a stable key. The key MUST NOT appear in any subsequent log entry.
- **FR-007**: Only the explicitly listed health endpoint (`/api/health`) is excluded from authentication requirements at all times. Exempted endpoints MUST return only liveness state (up/down) and MUST NOT expose version information, configuration, gateway connectivity state, or internal metrics. Adding new exempt paths requires a spec amendment.
- **FR-008**: The service MUST NOT log the active API key at any level after the initial FR-006 generation notice. Subsequent requests MUST NOT echo the key in logs.
- **FR-009**: Enabling or disabling authentication MUST require only a configuration file change and service restart — no database migration or state reset. Auth configuration is loaded once at startup; runtime config reloads MUST NOT affect the active key.
- **FR-010**: All `401` responses, regardless of failure mode (missing header, wrong key, malformed header), MUST return an identical response body containing the string "authentication required" and instructions to use the `Authorization: Bearer <api-key>` header. The body MUST NOT distinguish between failure modes, reveal the configured key, or expose internal details.
- **FR-011**: When authentication is enabled and a user-configured key loads successfully at startup, the service MUST log a single `INFO` line stating "API authentication enabled — using configured key" and the key length in characters.
- **FR-012**: When `require_auth = true` and the service is bound to a non-loopback address, the service MUST log a `WARN` at startup stating that API keys require transport encryption and that this process does not terminate TLS.
- **FR-013**: The in-memory API key MUST be zeroized (overwritten in memory) when the service shuts down.

### Key Entities

- **API Key**: The secret value used to authenticate client requests. Either user-supplied via configuration or auto-generated at startup using a CSPRNG. Never persisted to disk — lives in memory for the lifetime of the process and is zeroized on shutdown.
- **Auth Configuration**: The `[api]` section of the service configuration file, containing `require_auth` (boolean) and optionally `api_key` (string). Loaded once at startup; runtime changes require restart.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new user can enable API key protection by changing exactly one configuration value and restarting the service — no additional setup steps required.
- **SC-002**: A user who enables auth without setting a key can obtain a working key within 10 seconds of reading the startup log. (Test method: `docker compose logs | grep API_KEY_GENERATED` returns the key on a single line; the key works immediately in an `Authorization: Bearer` header.)
- **SC-003**: Existing deployments upgrading to this version experience zero behavioural change unless they explicitly set `require_auth = true` — no existing integrations break.
- **SC-004**: Auth validation latency is bounded and independent of key correctness — constant-time comparison ensures no timing information leaks through response time differences. (Test method: measure response time distributions for N correct-key requests vs N wrong-key requests; distributions must be statistically indistinguishable.)
- **SC-005**: 100% of `/api/*` routes are protected when auth is enabled; only explicitly listed exempt paths are accessible without an API key, regardless of configuration.

## Assumptions

- The service is the sole process managing the API key; no external identity provider is involved.
- A single shared API key is sufficient for v1 — multiple keys or per-client keys are out of scope.
- Key rotation requires a config change and restart; live key rotation without restart is out of scope.
- If an API key is compromised, the response procedure is: set a new key in config and restart the service. There is no revocation primitive beyond this.
- The reverse proxy (e.g., Caddy) handles TLS; the service itself does not terminate HTTPS. Users who enable auth without a TLS-terminating proxy will transmit API keys in cleartext — FR-012 warns them at startup.
- The startup log line containing the auto-generated API key is a secret. It MUST NOT be forwarded to external log aggregators (Loki, Datadog, journald remote sinks, etc.). Operators who require shipped logs should set an explicit key in config so the key never appears in log output.
- Auto-generated keys are not persisted between restarts by design. Any service restart generates a new key, breaking all clients until they recopy it. Users who need a stable key MUST set `api_key` in config.
- A Docker `restart: unless-stopped` policy combined with a misconfigured key will produce a restart loop. This is expected and intentional; the `ERROR` log emitted before exit identifies the cause.
- Rate limiting of failed authentication attempts is explicitly out of scope. Network-level controls (reverse proxy rules, firewall, Fail2ban) handle throttling.
- Character diversity validation on user-supplied keys was considered and deliberately excluded. A minimum length of 32 characters is the only enforced constraint — additional heuristics (entropy scoring, character-class checks) add user-visible friction without a corresponding customer ask, which conflicts with the North Star.
- The `/api/health` endpoint path is already defined and stable; this feature exempts only that path without changing it.
