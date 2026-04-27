# Troubleshooting

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
| Auto-generated key not visible | Log aggregator capturing Docker stderr | Use `docker compose logs \| grep "API_KEY_GENERATED:"` or set a static `api_key` |
