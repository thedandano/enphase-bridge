# Quickstart: Optional API Key Authentication

**Feature**: 002-api-key-auth | **Date**: 2026-04-26

## Default Behaviour (No Change Required)

Auth is disabled by default. Existing deployments continue working without any config changes.

```bash
curl http://localhost:8080/api/energy/windows
# 200 — works with no Authorization header
```

---

## Scenario 1: Enable Auth with Auto-Generated Key

**Step 1** — Add one line to `config.toml`:

```toml
[api]
host = "0.0.0.0"
port = 8080
require_auth = true
# api_key intentionally omitted — service generates one
```

**Step 2** — Restart the service:

```bash
docker compose restart
```

**Step 3** — Read the key from startup logs:

```bash
docker compose logs enphase-bridge | grep API_KEY_GENERATED
# {"event":"API_KEY_GENERATED","api_key":"aB3xK9mNpQ2rS5tU7vW0yZ4cE6fH8jL1","level":"WARN"}
```

**Step 4** — Use the key in requests:

```bash
API_KEY="aB3xK9mNpQ2rS5tU7vW0yZ4cE6fH8jL1"

curl -H "Authorization: Bearer $API_KEY" http://localhost:8080/api/energy/windows
# 200 — authenticated
```

**Important**: The auto-generated key changes on every restart. Set a static key (Scenario 2) if you need a stable key across restarts.

---

## Scenario 2: Enable Auth with a Static Key

**Step 1** — Generate a key (any method that produces ≥32 random characters):

```bash
openssl rand -base64 32
# e.g.: v8K2mP9nQ4rT7uW1xY3zA6bC0dF5gH8j (43 chars)
```

**Step 2** — Set it in `config.toml`:

```toml
[api]
host = "0.0.0.0"
port = 8080
require_auth = true
api_key = "v8K2mP9nQ4rT7uW1xY3zA6bC0dF5gH8j"
```

**Step 3** — Restart the service. The startup log confirms the key is loaded:

```json
{"event":"api_auth_enabled","key_len":43,"level":"INFO"}
```

The key is stable across restarts until you change the config.

---

## Scenario 3: Verify Auth is Working

```bash
# Without key — expect 401
curl -i http://localhost:8080/api/energy/windows
# HTTP/1.1 401 Unauthorized
# {"error":"authentication required","hint":"set Authorization: Bearer <api-key> header"}

# With wrong key — also 401 (identical body)
curl -i -H "Authorization: Bearer wrongkey" http://localhost:8080/api/energy/windows
# HTTP/1.1 401 Unauthorized

# Health check — always 200, no key needed
curl http://localhost:8080/api/health
# 200 {"status":"ok","uptime_secs":42}
```

---

## Scenario 4: Disable Auth

Remove `require_auth` from config (or set to `false`) and restart. All routes become open again — no other changes needed.

```toml
[api]
host = "0.0.0.0"
port = 8080
# require_auth omitted → defaults to false
```

---

## Scenario 5: Key Too Short (Startup Failure)

If `api_key` is set but shorter than 32 characters, the service refuses to start:

```json
{"event":"config_error","reason":"API key must be at least 32 characters; set a longer key or remove api_key from config to use auto-generation.","level":"ERROR"}
```

**Fix**: Use a key ≥32 characters, or remove `api_key` to let the service auto-generate one.

Under Docker's `restart: unless-stopped`, this produces a restart loop — intentional. Fix the config to stop the loop.

---

## Environment Variable Override

Config values can be overridden via environment variables using the `ENPHASE__` prefix:

```bash
ENPHASE__API__REQUIRE_AUTH=true
ENPHASE__API__API_KEY="your-key-here"
```

This is the recommended approach for container deployments — keep credentials out of `config.toml`.

---

## Security Notes

- **TLS**: API keys are sent in HTTP headers. Use a TLS-terminating reverse proxy (e.g. Caddy) to encrypt traffic. Without TLS, keys are sent in cleartext — the service logs a warning at startup if auth is enabled on a non-loopback address.
- **Log shipping**: The auto-generated key appears in the startup log (`API_KEY_GENERATED` event). Do not forward startup logs to external aggregators (Loki, Datadog, etc.) if using auto-generation. Use a static key instead.
- **Compromise response**: If your key is compromised, set a new key in config and restart.
