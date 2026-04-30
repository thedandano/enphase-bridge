# Data Contract

**Base URL**: `http://<host>:<port>` (default port 8080)  
**Format**: All bodies are `application/json`  
**Auth**: Disabled by default. When enabled, all routes except `/api/health` require `Authorization: Bearer <key>`.

---

## Common types

```
UnixTimestamp  integer   seconds since Unix epoch (UTC)
ISODateTime    string    RFC 3339, e.g. "2026-04-20T00:00:00Z"
```

Query parameters that accept dates use `ISODateTime`. Timestamps in responses are `UnixTimestamp`.

---

## Energy windows

### `GET /api/energy/windows`

Returns 15-minute energy windows for a time range.

**Query parameters**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `start` | ISODateTime | no | 24h ago | Range start (inclusive) |
| `end` | ISODateTime | no | now | Range end (inclusive) |
| `limit` | integer | no | 100 | Max records (capped at 2880 = 30 days) |
| `offset` | integer | no | 0 | Pagination offset |

**Response 200**

```json
{
  "windows": [
    {
      "window_start":   1745712000,
      "wh_produced":    423.5,
      "wh_consumed":    187.2,
      "wh_grid_import": 0.0,
      "wh_grid_export": 236.3,
      "is_complete":    true
    }
  ],
  "total":  96,
  "limit":  100,
  "offset": 0
}
```

| Field | Type | Description |
|-------|------|-------------|
| `window_start` | UnixTimestamp | Start of the 15-min bucket |
| `wh_produced` | float | Solar production in Wh (delta of `actEnergyDlvd` on production meter, EID 704643328) |
| `wh_consumed` | float | Site consumption in Wh — derived from energy balance: `wh_produced + wh_grid_import − wh_grid_export` (never negative) |
| `wh_grid_import` | float | Energy drawn from the grid in Wh (delta of `actEnergyDlvd` on net-consumption meter, EID 704643584; stalls at 0 during solar export — correct behaviour) |
| `wh_grid_export` | float | Energy exported to the grid in Wh (delta of `actEnergyRcvd` on net-consumption meter, EID 704643584; accumulates during solar export) |
| `is_complete` | boolean | `false` if the window is still in progress |

**Response 400** — invalid date or `start >= end`

```json
{ "error": "invalid_param", "message": "start must be before end" }
```

---

### `GET /api/energy/windows/latest`

Returns the single most recently completed 15-minute window (no pagination wrapper).

**Response 200** — same fields as a single window object above

**Response 404** — no data collected yet

```json
{ "error": "not_found", "message": "no windows recorded yet" }
```

---

## Inverter snapshots

### `GET /api/inverters/snapshots`

Per-microinverter power snapshots for a time range.

**Query parameters**

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `start` | ISODateTime | no | 24h ago | Range start (inclusive) |
| `end` | ISODateTime | no | now | Range end (inclusive) |
| `serial` | string | no | all | Filter to one inverter serial number |
| `limit` | integer | no | 200 | Max records |
| `offset` | integer | no | 0 | Pagination offset |

**Response 200**

```json
{
  "snapshots": [
    {
      "window_start":   1745712000,
      "serial_number":  "122201001234",
      "watts_output":   312.5,
      "is_online":      true
    }
  ],
  "total":  480,
  "limit":  200,
  "offset": 0
}
```

---

### `GET /api/inverters/snapshots/window/{window_start}`

All inverter snapshots for a single 15-minute window boundary.

**Path parameters**: `window_start` — Unix timestamp of the window boundary.

**Response 200**

```json
{
  "window_start": 1745712000,
  "inverters": [
    { "serial_number": "122201001234", "watts_output": 312.5, "is_online": true },
    { "serial_number": "122201001235", "watts_output": 305.0, "is_online": true },
    { "serial_number": "122201001236", "watts_output": 0.0,   "is_online": false }
  ]
}
```

**Response 404** — timestamp not found in storage

---

## Inverter arrays

### `GET /api/inverters/arrays`

Returns inverters grouped into named arrays (configured via `[arrays]` in `config.toml`), using the most recent snapshot. Inverters not yet seen in any snapshot are reported as `watts_output: 0.0, is_online: false`.

**No query parameters.**

**Response 200**

```json
{
  "window_start": 1745712000,
  "arrays": [
    {
      "name":         "south-roof",
      "total_watts":  1250.0,
      "online_count": 4,
      "total_count":  4,
      "inverters": [
        { "serial_number": "122201001234", "watts_output": 312.5, "is_online": true },
        { "serial_number": "122201001235", "watts_output": 305.0, "is_online": true }
      ]
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `window_start` | UnixTimestamp \| null | Timestamp of the snapshot used; `null` if no data collected yet |
| `arrays[].name` | string | Array name from config |
| `arrays[].total_watts` | float | Sum of watts across all inverters in the array |
| `arrays[].online_count` | integer | Number of inverters currently online |
| `arrays[].total_count` | integer | Total inverters configured in the array |

Arrays are sorted alphabetically by name.

---

## TOU rate management

### `POST /api/tou/refresh`

Fetches the current TOU rate schedule from OpenEI URDB and stores it as a new versioned entry.

**Request body**: empty

**Response 200**

```json
{
  "schedule_id":    4,
  "rate_label":     "TOU-DR Coastal Baseline Region",
  "effective_date": "2025-01-01",
  "fetched_at":     1745712300
}
```

**Response 502** — OpenEI API unreachable

```json
{ "error": "upstream_unavailable", "message": "OpenEI URDB API did not respond" }
```

---

## True-Up estimate

### `GET /api/trueup/estimate`

Computes (or returns cached) NEM true-up cost estimate for a date range.

**Query parameters**

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `start` | ISODateTime | yes | NEM anniversary period start |
| `end` | ISODateTime | yes | NEM anniversary period end |

**Response 200**

```json
{
  "period_start":  1714521600,
  "period_end":    1746057600,
  "net_cost_usd":  -142.37,
  "breakdown": {
    "peak": {
      "import_kwh":        182.4,
      "export_kwh":        310.1,
      "import_cost_usd":   72.96,
      "export_credit_usd": 81.24
    },
    "off_peak": {
      "import_kwh":        421.7,
      "export_kwh":        890.3,
      "import_cost_usd":   95.47,
      "export_credit_usd": 201.52
    },
    "super_off_peak": {
      "import_kwh":        53.2,
      "export_kwh":        120.8,
      "import_cost_usd":   8.51,
      "export_credit_usd": 19.33
    }
  },
  "tou_schedule": {
    "id":             3,
    "rate_label":     "TOU-DR Coastal Baseline Region",
    "effective_date": "2025-01-01"
  },
  "computed_at": 1745712300
}
```

`net_cost_usd` is negative when the period results in a credit.

**Response 400** — missing `start` or `end`

**Response 422** — no TOU schedule loaded (run `POST /api/tou/refresh` first)

```json
{ "error": "no_tou_schedule", "message": "no TOU rate schedule available; run POST /api/tou/refresh first" }
```

**Response 422** — no energy windows in the requested period

```json
{ "error": "insufficient_data", "message": "no energy windows found for the requested period" }
```

---

## Health

### `GET /api/health`

Liveness check. Always open — no auth required even when auth is enabled.

**Response 200**

```json
{
  "status":           "ok",
  "last_window_start": 1745712000,
  "token_expires_at":  1777248000,
  "uptime_seconds":    86400
}
```

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Always `"ok"` when the service is reachable |
| `last_window_start` | UnixTimestamp \| null | Most recent completed window; `null` if no data yet |
| `token_expires_at` | UnixTimestamp | When the gateway JWT expires |
| `uptime_seconds` | integer | Seconds since the process started |

---

## Error schema (all endpoints)

```json
{ "error": "<snake_case_code>", "message": "<human-readable description>" }
```

| HTTP status | When |
|-------------|------|
| 400 | Invalid input (bad date format, `start >= end`) |
| 404 | Requested resource not found |
| 422 | Valid input but request cannot be fulfilled (missing TOU schedule, no data) |
| 500 | Internal error (logged server-side) |
| 502 | Upstream dependency (OpenEI) unreachable |
