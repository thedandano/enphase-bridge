# enphase-bridge

[![CI](https://github.com/thedandano/enphase-bridge/actions/workflows/ci.yml/badge.svg)](https://github.com/thedandano/enphase-bridge/actions/workflows/ci.yml)
[![CD](https://github.com/thedandano/enphase-bridge/actions/workflows/cd.yml/badge.svg)](https://github.com/thedandano/enphase-bridge/actions/workflows/cd.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](./LICENSE)
[![Rust 2024](https://img.shields.io/badge/Rust-2024_edition-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-ghcr.io-2496ED?logo=docker&logoColor=white)](https://github.com/thedandano/enphase-bridge/pkgs/container/enphase-bridge)

A self-hosted Rust daemon that bridges your **Enphase IQ Gateway** to a local REST API. It polls your gateway on a configurable schedule, stores production and consumption data in SQLite, and serves a queryable HTTP API with optional Bearer token authentication.

Built for homeowners who want to own their energy data — run it on a Raspberry Pi, a NAS, or any small home server. Pair with Caddy for automatic HTTPS.

**📚 Documentation:** [Architecture](./docs/ARCHITECTURE.md) · [API Key Auth](./docs/API_KEY_AUTH.md) · [Data Contract](./docs/DATA_CONTRACT.md) · [Troubleshooting](./docs/TROUBLESHOOTING.md) · [Configuration](#configuration-reference) · [API Reference](#api-reference)

---

## Features

- **15-minute energy windows** — production, consumption, grid import/export (Wh)
- **Per-inverter snapshots** — power (W), online status, named array groupings
- **TOU cost estimation** — Time-of-Use peak/off-peak/super-off-peak breakdown using your utility's rates from the OpenEI Utility Rate Database (URDB)
- **True-Up estimate** — annual net metering cost or credit over any date range
- **Optional API key auth** — disabled by default; one config line to enable; auto-generated key or bring your own
- **Structured JSON logs** — every event has a machine-readable `event` field
- **Docker-ready** — single container, host networking, persistent volume for the database

---

## Quick start

### Prerequisites

| Requirement | Notes |
|-------------|-------|
| Enphase IQ Gateway | Reachable on your LAN at a known IP |
| Enphase gateway JWT | 1-year local access token — [how to get it](#enphase-gateway-jwt) |
| OpenEI API key + utility ID | Free — [sign up here](https://apps.openei.org/services/api/signup/) — [how to find both](#openei-api-key-utility-id--rate-label) |
| Docker | Any recent install; or Rust stable via [rustup.rs](https://rustup.rs) to build locally |

#### Enphase gateway JWT

1. Log in to [Enlighten](https://enlighten.enphaseenergy.com)
2. Open your system → **Settings** → **Local API Access** ([direct link](https://enlighten.enphaseenergy.com/app/settings/local-api-access))
3. Click **Generate token** — this produces a 1-year gateway-scoped JWT (not a cloud API key)
4. Copy the token for `ENPHASE__GATEWAY__TOKEN` (native config: `gateway.token`)

> **Note:** The Enlighten UI path changes occasionally. If you cannot find "Local API Access", search Enphase's community forums for the current path for your firmware version.

#### OpenEI API key, utility ID & rate label

1. Sign up for a free account at [OpenEI](https://apps.openei.org/services/api/signup/) and copy your API key
2. Use it for `ENPHASE__TOU__OPENEI_API_KEY` (native config: `tou.openei_api_key`)
3. Find your utility's **EIA ID** in the [OpenEI URDB](https://openei.org/wiki/Utility_Rate_Database): search by utility name and state, then look for the EIA ID in the utility details (e.g. `16609` for SDG&E, `14701` for PG&E, `3970` for SCE)
4. Use it for `ENPHASE__TOU__UTILITY_EIA_ID` (native config: `tou.utility_eia_id`)
5. Find your **rate plan name**: on the same URDB page, select your residential TOU plan and copy the exact **Name** field (e.g. `"TOU-DR Coastal Baseline Region"` for SDG&E, `"E-TOU-C"` for PG&E)
6. Use it for `ENPHASE__TOU__RATE_LABEL` (native config: `tou.rate_label`)

### 1. Clone and configure

For Docker deployments, continue to the [container examples](#2-run). This file-based setup is for native runs.

```bash
git clone https://github.com/thedandano/enphase-bridge.git
cd enphase-bridge
cp config.example.toml config.toml
echo "config.toml" >> .gitignore   # keep your credentials out of git
```

For a native run, edit `config.toml`. Container deployments can supply all required settings through environment variables instead:

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
timezone = "America/Los_Angeles" # Set your utility’s IANA timezone
openei_api_key = "your_openei_key"
utility_eia_id = 16609
rate_label     = "TOU-DR Coastal Baseline Region"
```

**Do not commit `config.toml`** — it contains your gateway token. Use environment variables (see [Configuration reference](#configuration-reference)) to pass secrets in containers.

### 2. Run

**Docker Compose (recommended):**

Create a `docker-compose.yml`. Supply `GATEWAY_TOKEN` and `OPENEI_API_KEY` through your shell or a git-ignored `.env` file beside it. Set the gateway host, utility ID, exact rate-plan name, and timezone for your installation:

```yaml
services:
  enphase-bridge:
    image: ghcr.io/thedandano/enphase-bridge:latest
    container_name: enphase-bridge
    restart: unless-stopped
    network_mode: host
    volumes:
      - enphase-data:/app/data
    environment:
      RUST_LOG: info
      ENPHASE__GATEWAY__HOST: "192.168.1.100"
      ENPHASE__GATEWAY__TOKEN: "${GATEWAY_TOKEN:?Set GATEWAY_TOKEN}"
      ENPHASE__POLLING__INTERVAL_SECS: "60"
      ENPHASE__API__HOST: "0.0.0.0"
      ENPHASE__API__PORT: "8080"
      ENPHASE__STORAGE__DB_PATH: /app/data/energy.db
      ENPHASE__TOU__OPENEI_API_KEY: "${OPENEI_API_KEY:?Set OPENEI_API_KEY}"
      ENPHASE__TOU__UTILITY_EIA_ID: "16609"
      ENPHASE__TOU__RATE_LABEL: "TOU-DR Coastal Baseline Region"
      ENPHASE__TOU__TIMEZONE: "America/Los_Angeles"

volumes:
  enphase-data:
```

```bash
docker compose up -d
docker compose logs -f
```

**Docker run (single container):**

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
  -e ENPHASE__TOU__UTILITY_EIA_ID="16609" \
  -e ENPHASE__TOU__RATE_LABEL="TOU-DR Coastal Baseline Region" \
  -e ENPHASE__TOU__TIMEZONE="America/Los_Angeles" \
  -v enphase-data:/data \
  ghcr.io/thedandano/enphase-bridge:latest
```

**Build and run locally:**

```bash
cargo build --release
./target/release/enphase-bridge
```

### 3. Load TOU rates

> Auth is not yet active — no `Authorization` header is required.

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

All routes return JSON. Auth is disabled by default — see [API Key Auth](./docs/API_KEY_AUTH.md) to enable it. Full request/response schemas are in [Data Contract](./docs/DATA_CONTRACT.md).

When auth is enabled, all routes except `/api/health` require a Bearer token:

```bash
curl -H "Authorization: Bearer <your-key>" http://localhost:8080/api/energy/windows
```

| Route | Method | Description |
|-------|--------|-------------|
| `/api/health` | GET | Liveness — always open, no auth required |
| `/api/energy/windows` | GET | 15-min energy windows — `?start`/`?end` RFC3339; `?formula_filter=current\|recomputable` |
| `/api/energy/windows/latest` | GET | Most recent completed window |
| `/api/power/samples` | GET | Raw instantaneous watts every 60s — `?start`/`?end`/`?limit`/`?offset` |
| `/api/power/phases` | GET | Per-phase (L1/L2) readings at each boundary — `?start`/`?end`/`?meter_eid` |
| `/api/inverters/snapshots` | GET | Per-inverter power snapshots |
| `/api/inverters/snapshots/window/{window_start}` | GET | Snapshots for a specific 15-min window |
| `/api/inverters/arrays` | GET | Inverters grouped into named arrays |
| `/api/tou/refresh` | POST | Fetch/refresh TOU revision history from OpenEI |
| `/api/tou/intervals` | GET | Read resolved bracket intervals (Unix start/end; maximum 31 days) |
| `/api/trueup/estimate` | GET | Net metering cost estimate — `?start`/`?end` RFC3339 |

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
| `api.require_auth` | — | **Planned, not yet active.** Bearer token auth is implemented but not wired into config. |
| `api.api_key` | — | **Planned, not yet active.** Static API key (≥32 chars); auto-generated if omitted. |
| `storage.db_path` | **required** | Path to the SQLite database file (e.g. `./energy.db`) |
| `tou.openei_api_key` | **required** | OpenEI API key for fetching TOU rate schedules |
| `tou.utility_eia_id` | **required** | Your utility's EIA ID in the OpenEI URDB (e.g. `16609` for SDG&E) |
| `tou.timezone` | **required** | Utility IANA timezone (e.g. `America/Los_Angeles`); use `ENPHASE__TOU__TIMEZONE` in containers |
| `tou.rate_label` | **required** | Rate plan name as listed in OpenEI URDB (e.g. `"TOU-DR Coastal Baseline Region"`) |
| `arrays.<name>` | _(none)_ | Named inverter array, e.g. `arrays.south_roof = ["122212345678", "122212345679"]` |

All keys can be overridden via environment variables using the `ENPHASE__` prefix with `__` as the section separator (e.g. `ENPHASE__API__PORT=9090`).

---

## Development

### Git hooks

After cloning, activate the repo's hooks once:

```bash
git config core.hooksPath .githooks
```

This runs `cargo fmt --check` + `cargo clippy` on every commit and `cargo test` on every push — the same checks CI enforces.

---

## License

[AGPL v3](./LICENSE) — free for personal and open-source use. Forks and derivatives must remain open source, including network-deployed services.

For commercial use, contact [dansedano.dev@gmail.com](mailto:dansedano.dev@gmail.com).

### No Warranty

This software is provided "as-is" without warranty of any kind, express or implied. The author assumes no liability for damages, data loss, or any other consequences resulting from the use of this software. You use enphase-bridge at your own risk.

### TOU timezone upgrade

In Docker Compose, add `ENPHASE__TOU__TIMEZONE: "America/Los_Angeles"` under the bridge service’s `environment` block. With `docker run`, add `-e ENPHASE__TOU__TIMEZONE=America/Los_Angeles`. Use your utility’s IANA timezone. Native TOML configuration uses `tou.timezone`; invalid or missing values stop startup. The bridge does not infer it from the plan or machine timezone.

### Resolved time-of-use intervals

`GET /api/tou/intervals?start=<epoch>&end=<epoch>` returns utility-local bracket timing as Unix seconds. Both parameters are required. The range must be positive and at most 31 days. Starts are inclusive and ends exclusive. Future transitions within a requested day are included.

```json
{
  "timezone": "America/Los_Angeles",
  "schedules": [{"id": 10, "source_id": "upstream-revision", "utility_eia_id": 16609, "rate_label": "Your plan", "effective_date": "2026-06-01", "effective_start": 1780297200, "effective_end": null}],
  "intervals": [{"start": 1791010800, "end": 1791061200, "bracket": "super_off_peak", "schedule_id": 10}]
}
```

Bracket names are `peak`, `off_peak`, and `super_off_peak`. Adjacent intervals merge only if both bracket and schedule identity match. Metadata end bounds include replacement by the next revision.

`POST /api/tou/refresh` now fetches all returned versions of the configured utility and exact plan name, including all pages, and saves them atomically. Its existing response fields remain, with an added `revision_count`. The interval GET only reads saved history. Failed refreshes leave it intact. A missing plan is an explicit error; names are never inferred.

Historical coverage comes from verified upstream revision identities and effective bounds, not fetch time. Legacy rows without provable metadata remain archived and cannot supply historical coverage. Revisions whose end precedes their start are archived with a warning and excluded. Explicit end dates are respected; gaps return HTTP 422 with `error: "tou_history_unavailable"`. Malformed schedules return HTTP 502 with `error: "upstream_parse_error"`.

Cost estimates use that same historical classification for each energy window. Existing `tou_schedule` identifies the last contributing revision; additive `tou_schedules` lists all contributing revisions. New saved estimates record all those IDs. Existing totals and the estimate endpoint's inclusive end-date convention are preserved. Neither endpoint invents holiday rules or reconstructs switches between different account plans.

Upgrading applies an additive migration that preserves existing schedules and estimates. Historical totals may change because past readings now use their effective revision rather than today's rates. Set `ENPHASE__TOU__TIMEZONE` in your Compose service or Docker arguments before recreating the container, then refresh history. Test the interval endpoint before enabling dashboard labels and transition lines. This change does not deploy the daemon.
