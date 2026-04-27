# enphase-bridge

[![CI](https://github.com/thedandano/enphase-bridge/actions/workflows/ci.yml/badge.svg)](https://github.com/thedandano/enphase-bridge/actions/workflows/ci.yml)
[![CD](https://github.com/thedandano/enphase-bridge/actions/workflows/cd.yml/badge.svg)](https://github.com/thedandano/enphase-bridge/actions/workflows/cd.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](./LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024_edition-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-ghcr.io-2496ED?logo=docker&logoColor=white)](https://github.com/thedandano/enphase-bridge/pkgs/container/enphase-bridge)

A self-hosted Rust daemon that bridges your **Enphase IQ Gateway** to a local REST API. It polls your gateway on a configurable schedule, stores production and consumption data in SQLite, and serves a queryable HTTP API with optional Bearer token authentication.

Built for homeowners who want to own their energy data — run it on a Raspberry Pi, a NAS, or any small home server. Pair with Caddy for automatic HTTPS.

**📚 Documentation:** [Architecture](./docs/ARCHITECTURE.md) · [API Key Auth](./docs/API_KEY_AUTH.md) · [Troubleshooting](./docs/TROUBLESHOOTING.md) · [Configuration](#configuration-reference) · [API Reference](#api-reference)

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
2. Open your system → **Settings** → **Local API Access** ([direct link](https://enlighten.enphaseenergy.com/app/settings/local-api-access))
3. Generate a **local** access token (valid 1 year for homeowner accounts). This is a gateway-scoped JWT, not a cloud API key.
4. Paste it into `config.toml` → `gateway.token`

> **Note:** The Enlighten UI path changes occasionally. If you cannot find "Local API Access", search Enphase's community forums for the current path for your firmware version.

#### Obtain an OpenEI API key and find your utility rate label

1. Sign up for a free account at [OpenEI](https://apps.openei.org/services/api/signup/)
2. Once registered, navigate to your [API Key](https://apps.openei.org/services/api/signup/) page and copy your key
3. Paste it into `config.toml` → `tou.openei_api_key`

4. Find your utility rate schedule label in the [OpenEI URDB](https://openei.org/wiki/Utility_Rate_Database):
   - Search for your utility name and state
   - Select your rate schedule (e.g., TOU, time-of-use, or dynamic pricing plan)
   - Copy the **Name** field (e.g., `"TOU-DR Coastal Baseline Region"` for SDG&E, or `"E-TOU-C"` for PG&E)
5. Paste it into `config.toml` → `tou.sdge_rate_label`

> The `sdge_rate_label` setting name is historical (SDG&E was the first supported utility), but it works with **any utility** in the OpenEI database. Use your utility's actual rate label, whatever it is.

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

Create a `docker-compose.yml` in your deployment directory:

```yaml
version: '3.8'

services:
  enphase-bridge:
    image: ghcr.io/thedandano/enphase-bridge:latest
    container_name: enphase-bridge
    restart: unless-stopped
    network_mode: host
    environment:
      ENPHASE__GATEWAY__HOST: "192.168.1.100"
      ENPHASE__GATEWAY__TOKEN: "eyJ..."
      ENPHASE__POLLING__INTERVAL_SECS: "60"
      ENPHASE__API__HOST: "0.0.0.0"
      ENPHASE__API__PORT: "8080"
      ENPHASE__STORAGE__DB_PATH: "/data/energy.db"
      ENPHASE__TOU__OPENEI_API_KEY: "your_openei_api_key"
      ENPHASE__TOU__SDGE_RATE_LABEL: "TOU-DR Coastal Baseline Region"
    volumes:
      - enphase-data:/data
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/api/health"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s

volumes:
  enphase-data:
    driver: local
```

Then start with:

```bash
docker compose up -d
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

All routes return JSON. Auth is disabled by default — see [API Key Auth](./docs/API_KEY_AUTH.md) to enable it.

When auth is enabled, all routes except `/api/health` require a Bearer token:

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

## License

[AGPL v3](./LICENSE) — free for personal and open-source use. Forks and derivatives must remain open source, including network-deployed services.

For commercial use, contact [dansedano.dev@gmail.com](mailto:dansedano.dev@gmail.com).
