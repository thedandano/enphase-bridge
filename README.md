# enphase-bridge

[![CI](https://github.com/thedandano/enphase-bridge/actions/workflows/ci.yml/badge.svg)](https://github.com/thedandano/enphase-bridge/actions/workflows/ci.yml)
[![CD](https://github.com/thedandano/enphase-bridge/actions/workflows/cd.yml/badge.svg)](https://github.com/thedandano/enphase-bridge/actions/workflows/cd.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](./LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024_edition-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-ghcr.io-2496ED?logo=docker&logoColor=white)](https://github.com/thedandano/enphase-bridge/pkgs/container/enphase-bridge)

A self-hosted Rust daemon that bridges your **Enphase IQ Gateway** to a local REST API. It polls your gateway on a configurable schedule, stores production and consumption data in SQLite, and serves a queryable HTTP API with optional Bearer token authentication.

Built for homeowners who want to own their energy data — run it on a Raspberry Pi, a NAS, or any small home server. Pair with Caddy for automatic HTTPS.

> For architecture diagrams, component details, and technology choices, see [ARCH.md](./ARCH.md).

---

## Features

- **15-minute energy windows** — production, consumption, grid import/export (Wh)
- **Per-inverter snapshots** — power (W), online status, named array groupings
- **TOU cost estimation** — Time-of-Use peak/off-peak/super-off-peak breakdown against SDG&E rates from the OpenEI Utility Rate Database (URDB)
- **True-Up estimate** — annual net metering cost or credit over any date range
- **Optional API key auth** — disabled by default; one config line to enable; auto-generated key or bring your own
- **Structured JSON logs** — every event has a machine-readable `event` field
- **Docker-ready** — single container, host networking, persistent volume for the database

---

## Quick start

### Prerequisites

| Requirement | Notes |
|-------------|-------|
| Enphase IQ Gateway | Reachable on your LAN |
| Enphase Enlighten account | Needed to generate a local JWT |
| OpenEI API key | Free — [sign up here](https://apps.openei.org/services/api/signup/) |
| Rust stable (or Docker) | Install Rust via [rustup.rs](https://rustup.rs) or use any recent Docker install |

### 1. Clone and configure

```bash
git clone https://github.com/thedandano/enphase-bridge.git
cd enphase-bridge
cp config.example.toml config.toml
echo "config.toml" >> .gitignore   # keep your credentials out of git
```

Edit `config.toml`:

```toml
[gateway]
host  = "192.168.1.100"   # IQ Gateway LAN IP (preferred over "envoy.local" on Linux)
token = "eyJ..."           # Local JWT from Enlighten (see below)

[polling]
interval_secs = 60         # Poll interval — minimum 15

[api]
host = "0.0.0.0"
port = 8080

[storage]
db_path = "./energy.db"

[tou]
openei_api_key  = "your_openei_key"
sdge_rate_label = "TOU-DR Coastal Baseline Region"
```

**Do not commit `config.toml`** — it contains your gateway token. Use environment variables (see [Via environment variables](#via-environment-variables)) to pass secrets in containers.

#### Obtain a gateway JWT

1. Log in to [Enlighten](https://enlighten.enphaseenergy.com)
2. Open your system → **Settings** → **Local API Access**
3. Generate a **local** access token (valid 1 year for homeowner accounts). This is a gateway-scoped JWT, not a cloud API key.
4. Paste it into `config.toml` → `gateway.token`

> **Note:** The Enlighten UI path changes occasionally. If you cannot find "Local API Access", search Enphase's community forums for the current path for your firmware version.

### 2. Run

**From source:**

```bash
cargo build --release
./target/release/enphase-bridge
```

**Docker (single container):**

```bash
docker run -d \
  --name enphase-bridge \
  --network host \
  -e ENPHASE__GATEWAY__HOST="192.168.1.100" \
  -e ENPHASE__GATEWAY__TOKEN="eyJ..." \
  -e ENPHASE__POLLING__INTERVAL_SECS="60" \
  -e ENPHASE__API__HOST="0.0.0.0" \
  -e ENPHASE__API__PORT="8080" \
  -e ENPHASE__STORAGE__DB_PATH="/data/energy.db" \
  -e ENPHASE__TOU__OPENEI_API_KEY="your_openei_key" \
  -e ENPHASE__TOU__SDGE_RATE_LABEL="TOU-DR Coastal Baseline Region" \
  -v enphase-data:/data \
  ghcr.io/thedandano/enphase-bridge:latest
```

**Docker Compose (recommended for production):**

```bash
# GITHUB_REPOSITORY controls which image is pulled:
# ghcr.io/<owner>/enphase-bridge:latest
GITHUB_REPOSITORY=thedandano/enphase-bridge docker compose up -d
docker compose logs -f
```

### 3. Load TOU rates

> These examples assume the default (auth disabled). If you enabled `require_auth`, add `-H "Authorization: Bearer <your-key>"` to every request.

```bash
curl -X POST http://localhost:8080/api/tou/refresh
```

### 4. Verify data is flowing

After one polling cycle:

```bash
curl http://localhost:8080/api/energy/windows/latest
curl http://localhost:8080/api/health
```

---

## API reference

All routes return JSON. Auth is disabled by default — see [Optional API key auth](#optional-api-key-auth) to protect them.

When `require_auth = true`, all routes except `/api/health` require a Bearer token:

```bash
curl -H "Authorization: Bearer <your-key>" http://localhost:8080/api/energy/windows
```

| Route | Method | Description |
|-------|--------|-------------|
| `/api/health` | GET | Liveness — always open, no auth required |
| `/api/energy/windows` | GET | 15-min energy windows (filterable by `start`/`end` — RFC3339 UTC) |
| `/api/energy/windows/latest` | GET | Most recent completed window (partial in-progress windows excluded) |
| `/api/inverters/snapshots` | GET | Per-inverter power snapshots |
| `/api/inverters/snapshots/window/{window_start}` | GET | Snapshots for a specific window timestamp (RFC3339 UTC) |
| `/api/inverters/arrays` | GET | Inverters grouped into named arrays |
| `/api/tou/refresh` | POST | Fetch/refresh TOU rate schedule from OpenEI |
| `/api/trueup/estimate` | GET | Net metering cost estimate (`start`/`end` — RFC3339 UTC) |

**Example — last 7 days of energy:**

```bash
curl "http://localhost:8080/api/energy/windows?start=2026-04-20T00:00:00Z&end=2026-04-27T00:00:00Z"
```

**Example — annual True-Up estimate:**

```bash
curl "http://localhost:8080/api/trueup/estimate?start=2025-04-27T00:00:00Z&end=2026-04-27T00:00:00Z"
```

---

## Optional API key auth

Auth is **off by default**. All routes are open — no change required for existing LAN deployments.

### Enable with auto-generated key

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

### Enable with a static key

```toml
[api]
require_auth = true
api_key = "your-key-at-least-32-characters-long"
```

Rules: minimum 32 characters. Empty or whitespace-only values are treated as absent (auto-generate). Keys shorter than 32 characters cause the service to refuse to start (exit code 2).

### Via environment variables

```bash
ENPHASE__API__REQUIRE_AUTH=true
ENPHASE__API__API_KEY="your-key-here"
```

Recommended for containers — keeps credentials out of `config.toml`.

### Key rotation

If your key is compromised:

1. Set a new key in `config.toml` (or via `ENPHASE__API__API_KEY`) and restart.
2. Purge the old key from shell history (`history -d` or `~/.zsh_history`).
3. Check any reverse-proxy access logs, log aggregators, or monitoring exporters that may have captured requests with the old token.

### Security notes

- **Always use TLS** when exposing the service beyond your own device. Bearer tokens are sent in cleartext HTTP headers — anyone on the same broadcast domain (including IoT devices, guest Wi-Fi) can intercept them. Use a TLS-terminating reverse proxy such as [Caddy](https://caddyserver.com) or Tailscale. The service logs a warning at startup if auth is enabled on a non-loopback address.
- **Static key preferred when forwarding logs.** When running under Docker, the auto-generated key is emitted to stderr, which Docker's log driver captures. Any log aggregator (Loki, journald, awslogs, etc.) attached to the container will receive it. Use a static key via environment variable instead.
- **Treat `gateway.token` as a long-lived credential.** It grants full local gateway access and is valid for up to 1 year. If leaked, regenerate it in Enlighten (the old token remains valid until expiry — Enphase does not currently support revocation). Do not commit `config.toml`.

---

## Docker deployment

```bash
# Clone the repo if you haven't already
git clone https://github.com/thedandano/enphase-bridge.git
cd enphase-bridge

# GITHUB_REPOSITORY is interpolated into the image reference in compose.yaml:
# image: ghcr.io/<owner>/enphase-bridge:latest
GITHUB_REPOSITORY=thedandano/enphase-bridge docker compose up -d

# View logs
docker compose logs -f enphase-bridge

# Restart after config change
docker compose restart enphase-bridge
```

The container uses `network_mode: host` so it can reach your IQ Gateway at its LAN IP. The SQLite database is stored on a named volume (`enphase-data`) and survives container restarts.

---

## Configuration reference

| Key | Default | Description |
|-----|---------|-------------|
| `gateway.host` | **required** | IQ Gateway LAN IP or hostname |
| `gateway.token` | **required** | Local JWT from Enlighten |
| `polling.interval_secs` | **required** | Poll interval in seconds (min: 15) |
| `api.host` | **required** | Bind address for the HTTP server (e.g. `0.0.0.0`) |
| `api.port` | **required** | Port for the HTTP server (e.g. `8080`) |
| `api.require_auth` | `false` | Enable Bearer token auth |
| `api.api_key` | _(none)_ | Static API key (≥32 chars); consulted only when `require_auth = true`; omit to auto-generate |
| `storage.db_path` | **required** | Path to the SQLite database file (e.g. `./energy.db`) |
| `tou.openei_api_key` | **required** | OpenEI API key for fetching TOU rate schedules |
| `tou.sdge_rate_label` | **required** | Rate label to match in OpenEI URDB |
| `arrays.<name>` | _(none)_ | Named inverter array, e.g. `arrays.south_roof = ["122212345678", "122212345679"]` |

All keys can be overridden via environment variables using the `ENPHASE__` prefix with `__` as the section separator (e.g. `ENPHASE__API__PORT=9090`).

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `auth_error` in logs + exit 1 | Gateway token expired | Re-generate in Enlighten → update `config.toml` |
| `config_error` in logs + exit 2 | `api_key` shorter than 32 chars | Use a longer key or remove it to auto-generate |
| Exit code 3 | Runtime error (DB, network) | Check logs for the preceding error event |
| Connection refused to gateway | Wrong IP or mDNS not resolving | Check `gateway.host`; prefer the IP form over `envoy.local` on Linux (requires Avahi) |
| No data after one polling interval | Poll failing silently | Check logs for `poll_error` events |
| Inverter `is_online: false` | Inverter not reporting | Check Enlighten for device status |
| `502` from `/api/tou/refresh` | OpenEI API down or bad key | Verify `tou.openei_api_key` |
| 401 on all routes | Auth enabled, missing header | Add `-H "Authorization: Bearer <key>"` to requests |
| Auto-generated key not visible | Log aggregator capturing Docker stderr | Use `docker compose logs | grep "API_KEY_GENERATED:"` or set a static `api_key` |

---

## License

[AGPL v3](./LICENSE) — free for personal and open-source use. Forks and derivatives must remain open source, including network-deployed services.

For commercial use, contact [dansedano.dev@gmail.com](mailto:dansedano.dev@gmail.com).
