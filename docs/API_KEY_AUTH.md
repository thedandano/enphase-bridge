# Optional API Key Authentication

Auth is **off by default**. All routes are open — no change required for existing LAN deployments.

## Enable with auto-generated key

Add one line to `config.toml` and restart:

```toml
[api]
host = "0.0.0.0"
port = 8080
require_auth = true
# api_key omitted — service generates one at startup
```

The auto-generated key (43-character base64url, 32 bytes of entropy) is written to **stderr** at startup:

```bash
docker compose logs enphase-bridge | grep "API_KEY_GENERATED:"
# [enphase-bridge] API_KEY_GENERATED: aB3x...43chars
```

> **Important:** Docker captures stderr into its log driver (the same place `docker compose logs` reads from). Any log aggregator attached to the container — Loki, journald, awslogs, Datadog — will receive the auto-generated key. **Use a static `api_key` set via environment variable if you forward container logs to an external system.**

Use it in requests:

```bash
curl -H "Authorization: Bearer aB3x...43chars" http://localhost:8080/api/energy/windows
```

> The auto-generated key changes on every restart. Set `api_key` in config for a stable key.

## Enable with a static key

```toml
[api]
require_auth = true
api_key = "your-key-at-least-32-characters-long"
```

Rules: minimum 32 characters. Empty or whitespace-only values are treated as absent (auto-generate). Keys shorter than 32 characters cause the service to refuse to start (exit code 2).

## Via environment variables

```bash
ENPHASE__API__REQUIRE_AUTH=true
ENPHASE__API__API_KEY="your-key-here"
```

Recommended for containers — keeps credentials out of `config.toml`.

## Key rotation

If your key is compromised:

1. Set a new key in `config.toml` (or via `ENPHASE__API__API_KEY`) and restart.
2. Purge the old key from shell history (`history -d` or `~/.zsh_history`).
3. Check any reverse-proxy access logs, log aggregators, or monitoring exporters that may have captured requests with the old token.

## Security notes

- **Always use TLS** when exposing the service beyond your own device. Bearer tokens are sent in cleartext HTTP headers — anyone on the same broadcast domain (including IoT devices, guest Wi-Fi) can intercept them. Use a TLS-terminating reverse proxy such as [Caddy](https://caddyserver.com) or Tailscale. The service logs a warning at startup if auth is enabled on a non-loopback address.
- **Static key preferred when forwarding logs.** When running under Docker, the auto-generated key is emitted to stderr, which Docker's log driver captures. Any log aggregator (Loki, journald, awslogs, etc.) attached to the container will receive it. Use a static key via environment variable instead.
- **Treat `gateway.token` as a long-lived credential.** It grants full local gateway access and is valid for up to 1 year. If leaked, regenerate it in Enlighten (the old token remains valid until expiry — Enphase does not currently support revocation). Do not commit `config.toml`.
