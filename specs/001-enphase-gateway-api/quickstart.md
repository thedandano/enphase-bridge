# Quickstart: Enphase Gateway Data Service

**Branch**: `001-enphase-gateway-api` | **Date**: 2026-04-26

---

## Prerequisites

- Rust stable toolchain (via `rustup`)
- Your Enphase IQ Gateway reachable on the LAN (find its IP from your router)
- Enphase Enlighten account credentials (to obtain a local JWT)
- OpenEI API key (free signup: https://apps.openei.org/services/api/signup/)

---

## 1. Configuration

Copy the example config and edit it:

```bash
cp config.example.toml config.toml
```

`config.toml`:

```toml
[gateway]
host = "192.168.1.100"        # Your IQ Gateway LAN IP (or "envoy.local")
token = "eyJ..."              # JWT from Enphase cloud — see step 2

[polling]
interval_secs = 60            # How often to poll the gateway (min: 15)

[api]
host = "0.0.0.0"
port = 8080

[storage]
db_path = "./energy.db"

[tou]
openei_api_key = "your_key_here"
sdge_rate_label = "NEM 2.0 TOU-DR"   # Match the label from OpenEI for your tariff
```

**Do not commit `config.toml`** — it contains your gateway token. It is in `.gitignore`.

---

## 2. Obtain a Gateway JWT

The Enphase IQ Gateway requires a JWT obtained from the Enphase cloud:

1. Log in to Enlighten: https://enlighten.enphaseenergy.com
2. Go to your system → Settings → Local API Access
3. Generate a local access token (valid 1 year for homeowner accounts)
4. Paste the token into `config.toml` under `gateway.token`

---

## 3. Build and Run

```bash
# Build (release mode recommended for the daemon)
cargo build --release

# Run
./target/release/enphase-ds
```

The service will:
1. Connect to the gateway and verify auth
2. Load the inverter inventory (serial numbers)
3. Start polling every `interval_secs`
4. Serve the API at `http://0.0.0.0:8080`

---

## 4. Load SDGE TOU Rates

On first run (or when SDGE updates rates), fetch the rate schedule:

```bash
curl -X POST http://localhost:8080/api/tou/refresh
```

Expected response:
```json
{"schedule_id": 1, "rate_label": "NEM 2.0 TOU-DR", "effective_date": "2025-01-01", ...}
```

---

## 5. Verify Data Collection

After one polling cycle (default 60s), confirm data is flowing:

```bash
# Latest 15-minute window
curl http://localhost:8080/api/energy/windows/latest

# Last 24 hours of windows
curl "http://localhost:8080/api/energy/windows?start=2026-04-25T00:00:00Z"

# Service health
curl http://localhost:8080/api/health
```

---

## 6. Run Tests

```bash
# All tests
cargo test

# Integration tests only (requires gateway reachable or mock)
cargo test --test integration

# Unit tests only
cargo test --lib
```

---

## 7. Estimate Your True-Up

After collecting at least a few days of data:

```bash
curl "http://localhost:8080/api/trueup/estimate?start=2025-04-26T00:00:00Z&end=2026-04-26T00:00:00Z"
```

The response includes a breakdown by TOU period (peak / off-peak / super-off-peak) and a net cost or credit.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `401` from gateway | Token expired or invalid | Re-generate token in Enlighten, update `config.toml` |
| `connection refused` to gateway | Wrong IP | Check `gateway.host` in config; ping the gateway |
| `502` from `/api/tou/refresh` | OpenEI API down or bad key | Verify `openei_api_key` in config |
| Inverter showing `is_online: false` | Inverter not reporting | Check Enlighten for device status; may be a hardware issue |
| No data after 15 min | Window aggregator not completing | Check logs for poll failures (`is_complete` will be `false`) |
