## Context

The previous fix (commit c0ff1ff) introduced EID `1023410688` as the source for grid import/export cumulative counters. This EID does not appear in the official Enphase IQ Gateway Local APIs tech brief (Jan 2023). The documented gateway exposes exactly two top-level meters in `/ivp/meters/readings`:

- `704643328` (`measurementType: "production"`) — solar production CT
- `704643584` (`measurementType: "net-consumption"`) — grid-connection CT, bidirectional

The net-consumption meter (EID_CONSUMPTION) already carries both counters the bridge needs:
- `actEnergyDlvd` — lifetime Wh delivered from grid to house (import)
- `actEnergyRcvd` — lifetime Wh received from house by grid (export)

Because `find(|m| m.eid == EID_NET)` returns `None` on every poll, both grid counters default to `0.0`, and the energy balance degenerates to `consumed = produced`. The `eid_net_absent` warning fires on every poll but is easily missed.

## Goals / Non-Goals

**Goals:**
- Source `grid_import_cum_wh` from `actEnergyDlvd[EID_CONSUMPTION]` and `grid_export_cum_wh` from `actEnergyRcvd[EID_CONSUMPTION]` — the only documented bidirectional counters on this gateway.
- Add a startup probe (`GET /ivp/meters`) to validate required meter presence before polling begins; fail fast and loud if absent.
- Eliminate silent `0.0` fallback for missing meter data.
- Update all test fixtures and add regression tests covering both Bug v1 (import stalls during export) and Bug v2 (consumed equals produced).
- Update documentation to reflect the corrected data source.

**Non-Goals:**
- Switching `consumption_w_now` from the current source (a separate improvement; tracked separately).
- Adding `/ivp/livedata/status` for direct load measurement (separate change).
- Backfilling previously incorrect historical windows.
- Supporting battery/storage EID configurations (out of scope per original spec).

## Decisions

### Decision 1: Use EID_CONSUMPTION (`704643584`) for both bidirectional grid counters

**Chosen**: Read `actEnergyDlvd` and `actEnergyRcvd` from EID_CONSUMPTION for grid import and export respectively.

**Rationale**: This is the only officially documented bidirectional meter. Its `actEnergyDlvd` tracks grid→house imports (stalls during export, which is correct — imports stop when solar covers all load). Its `actEnergyRcvd` tracks house→grid exports (accumulates during solar export). Together with the energy balance formula (`consumed = produced + import - export`), both Bug v1 and Bug v2 are resolved.

**Alternatives considered**:
- Keep EID_NET with EID_CONSUMPTION fallback: rejected — EID_NET is not a valid meter EID in any documented firmware. Retaining it as a "preferred" source would silently break again on any gateway that matches the documented spec.
- Keep EID_NET only: rejected — this is what caused Bug v2.

### Decision 2: Add `GET /ivp/meters` startup probe

**Chosen**: Call `GET /ivp/meters` once at scheduler startup (after `check_jwt`), parse the meter list, and verify a `net-consumption` meter with `state: "enabled"` exists. If absent, return `GatewayError` and halt.

**Rationale**: Hardcoded EID constants are the root cause of both Bug v1 and Bug v2. Discovering meters by `measurementType` at runtime is more robust and provides an explicit, observable error instead of silent zero-data. The startup cost is one extra HTTP request.

**Alternatives considered**:
- Keep hardcoded EIDs, add `cons.is_none()` error: simpler but still fragile — a future firmware change that renames the EID would silently break again without the probe logging which meters were actually returned.

### Decision 3: Elevate missing net-consumption meter to `GatewayError`, not `warn!` with `0.0`

**Chosen**: If the startup probe or poll finds `cons.is_none()`, return `Err(GatewayError::MissingMeter("net-consumption"))`. The scheduler handles this as a poll error (logged, skipped window).

**Rationale**: Defaulting missing meters to `0.0` is a silent failure that writes persistently wrong windows. An error causes the window to be skipped (recoverable) and logged (observable), which is correct behavior when the gateway's meter configuration is unexpected.

### Decision 4: Keep `EID_NET` constant only for `grid_w_now`

**Chosen**: Retain `EID_NET` (`1023410688`) for the real-time `grid_w_now` field (`activePower` on the net meter, where present). This field is informational, not used in window math. Remove it entirely for the cumulative counter lookups.

**Rationale**: Some gateway firmware versions may expose EID_NET for instantaneous readings even when its cumulative counters are zero. Keeping it for `grid_w_now` is a graceful optional-source pattern (defaults to `0.0` if absent, which is acceptable for a display-only field).

## Risks / Trade-offs

- **Startup probe adds one extra authenticated request**: `GET /ivp/meters` runs once at startup after `check_jwt`. On a 10-second startup, this is negligible. If the endpoint returns 401, it will be caught by the same retry logic as other endpoints.
- **`actEnergyRcvd[CONS]` semantics depend on CT installation direction**: For standard Enphase installations the CT is clamped at the service entrance with the conventional direction. An inverted CT would swap import/export semantics. This is a physical installation issue outside the software's control; no mitigation is practical without a hardware detection mechanism.
- **One poll cycle gap on restart**: Existing behavior — `load_persisted_reading` returns `None` if keys are absent, scheduler skips the first window boundary.

## Migration Plan

1. Deploy updated binary — no DB migration, no config changes.
2. On first startup, the probe calls `/ivp/meters` and confirms EID_CONSUMPTION is present with `state: "enabled"`.
3. First poll reads both `actEnergyDlvd` and `actEnergyRcvd` from EID_CONSUMPTION; `persist_reading` writes the baseline.
4. All subsequent windows use the corrected counters.
5. Historical windows stored by the broken c0ff1ff code remain wrong (consumed = produced). These resolve naturally as correct windows accumulate; no backfill needed for daily totals after the next UTC midnight.
