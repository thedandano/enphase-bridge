# API Contract: Enphase Gateway Data Service

**Branch**: `001-enphase-gateway-api` | **Date**: 2026-04-26
**Type**: HTTP REST (JSON, read-only except TOU refresh)
**Base URL**: `http://<host>:8080` (LAN, no auth)
**Format**: All request/response bodies are `application/json`

---

## Common Types

```typescript
// ISO8601 timestamps accepted as query params: "2026-04-01T00:00:00Z"
// Unix timestamps in responses: integer seconds since epoch (UTC)

type UnixTimestamp = number;   // seconds since epoch
type ISODateString = string;   // "YYYY-MM-DDTHH:MM:SSZ"
```

---

## Energy Windows

### `GET /api/energy/windows`

Returns 15-minute energy windows for a time range.

**Query Parameters**:

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `start` | ISO8601 | no | 24h ago | Window start inclusive |
| `end` | ISO8601 | no | now | Window end inclusive |
| `limit` | integer | no | 100 | Max records returned (max: 2880 = 30 days) |
| `offset` | integer | no | 0 | Pagination offset |

**Response 200**:
```json
{
  "windows": [
    {
      "window_start": 1745712000,
      "wh_produced": 423.5,
      "wh_consumed": 187.2,
      "wh_grid_import": 0.0,
      "wh_grid_export": 236.3,
      "is_complete": true
    }
  ],
  "total": 96,
  "limit": 100,
  "offset": 0
}
```

**Response 400** (invalid range):
```json
{ "error": "invalid_range", "message": "start must be before end" }
```

---

### `GET /api/energy/windows/latest`

Returns the most recently completed 15-minute window.

**Response 200**: Single window object (same shape as above, unwrapped — no pagination wrapper).

**Response 404** (no data collected yet):
```json
{ "error": "no_data", "message": "no windows recorded yet" }
```

---

## Microinverter Snapshots

### `GET /api/inverters/snapshots`

Returns per-microinverter snapshots for a time range.

**Query Parameters**:

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `start` | ISO8601 | no | 24h ago | Window start inclusive |
| `end` | ISO8601 | no | now | Window end inclusive |
| `serial` | string | no | all | Filter to one microinverter serial |
| `limit` | integer | no | 200 | Max records returned |
| `offset` | integer | no | 0 | Pagination offset |

**Response 200**:
```json
{
  "snapshots": [
    {
      "window_start": 1745712000,
      "serial_number": "122201001234",
      "watts_output": 312.5,
      "is_online": true
    }
  ],
  "total": 480,
  "limit": 200,
  "offset": 0
}
```

---

### `GET /api/inverters/snapshots/window/{window_start}`

Returns all microinverter snapshots for a single 15-minute window.

**Path Parameters**: `window_start` — Unix timestamp of window boundary.

**Response 200**:
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

**Response 404**: Window timestamp not found in storage.

---

## TOU True-Up Estimation

### `GET /api/trueup/estimate`

Computes (or retrieves cached) NEM true-up estimate for a date range.

**Query Parameters**:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `start` | ISO8601 | yes | NEM anniversary period start |
| `end` | ISO8601 | yes | NEM anniversary period end |

**Response 200**:
```json
{
  "period_start": 1714521600,
  "period_end": 1746057600,
  "net_cost_usd": -142.37,
  "breakdown": {
    "peak": {
      "import_kwh": 182.4,
      "export_kwh": 310.1,
      "import_cost_usd": 72.96,
      "export_credit_usd": 81.24
    },
    "off_peak": {
      "import_kwh": 421.7,
      "export_kwh": 890.3,
      "import_cost_usd": 95.47,
      "export_credit_usd": 201.52
    },
    "super_off_peak": {
      "import_kwh": 53.2,
      "export_kwh": 120.8,
      "import_cost_usd": 8.51,
      "export_credit_usd": 19.33
    }
  },
  "tou_schedule": {
    "id": 3,
    "rate_label": "NEM 2.0 TOU-DR",
    "effective_date": "2025-01-01"
  },
  "computed_at": 1745712300
}
```

**Response 400** (missing params):
```json
{ "error": "missing_param", "message": "start and end are required" }
```

**Response 422** (no TOU schedule loaded):
```json
{ "error": "no_tou_schedule", "message": "no TOU rate schedule available; run POST /api/tou/refresh first" }
```

**Response 422** (insufficient energy data):
```json
{ "error": "insufficient_data", "message": "no energy windows found for the requested period" }
```

---

## TOU Rate Management

### `POST /api/tou/refresh`

Triggers a fetch of the current SDGE TOU rate schedule from OpenEI URDB and stores it as a new versioned schedule.

**Request body**: empty

**Response 200**:
```json
{
  "schedule_id": 4,
  "rate_label": "NEM 2.0 TOU-DR",
  "effective_date": "2025-01-01",
  "fetched_at": 1745712300
}
```

**Response 502** (OpenEI API unreachable):
```json
{ "error": "upstream_unavailable", "message": "OpenEI URDB API did not respond" }
```

---

## Service Status

### `GET /api/health`

Lightweight liveness check.

**Response 200**:
```json
{
  "status": "ok",
  "last_window_start": 1745712000,
  "token_expires_at": 1777248000,
  "uptime_seconds": 86400
}
```

---

## Error Response Schema (all endpoints)

```json
{
  "error": "<snake_case_code>",
  "message": "<human-readable description>"
}
```

HTTP status codes used: `200`, `400` (bad input), `404` (not found), `422` (unprocessable — valid input but can't fulfil), `502` (upstream failure), `500` (internal error).
