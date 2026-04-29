## Context

The Enphase IQ Gateway exposes `/ivp/meters/readings` which returns per-meter objects. Each object has two cumulative energy counters: `actEnergyDlvd` (energy delivered in the "forward" direction) and `actEnergyRcvd` (energy received in the "reverse" direction). The bridge currently only deserializes `actEnergyDlvd`.

For the production meter this is correct — solar only flows one way. But for the net/consumption meter, `actEnergyDlvd` tracks only grid → house imports. Grid exports (house → grid) accumulate in `actEnergyRcvd`, which the bridge never reads. As a result, `wh_consumed` in `compute_delta` is computed as `delta(actEnergyDlvd[consumption meter])`, which stalls at zero the moment solar covers all load and begins exporting.

The current data flow:

```
Gateway JSON
  actEnergyDlvd[PROD]  → MeterReadings.production_cum_wh
  actEnergyDlvd[CONS]  → MeterReadings.consumption_cum_wh   ← stalls on export

CumulativeReading
  .production_wh  = production_cum_wh
  .consumption_wh = consumption_cum_wh                      ← stalls on export

compute_delta
  wh_produced     = delta(production_wh)                    ← correct
  wh_consumed     = delta(consumption_wh)                   ← WRONG: 0 during export
  wh_grid_export  = (wh_produced - wh_consumed).max(0)      ← numerically consistent but wrong
  wh_grid_import  = (wh_consumed - wh_produced).max(0)      ← wrong
```

## Goals / Non-Goals

**Goals:**
- `wh_consumed` reflects actual house load in all windows, including during solar export.
- `wh_grid_import` and `wh_grid_export` are independently derived from net-meter counters, not each other.
- The fix works for both net-metering setups (no dedicated consumption CT) and full consumption monitoring setups.
- No change to the `energy_window` DB schema or REST API response shape.

**Non-Goals:**
- Supporting Enphase configurations with storage/battery (different EID set; out of scope).
- Providing a backfill mechanism to correct already-stored incorrect windows.
- Validating or choosing between direct consumption measurement vs. energy balance for systems that have both.

## Decisions

### Decision 1: Use energy balance to derive `wh_consumed` rather than reading `actEnergyDlvd[CONS]` directly

**Chosen**: Derive `wh_consumed = wh_produced + wh_grid_import - wh_grid_export` using the net meter's bidirectional counters.

**Alternative considered**: Continue reading `actEnergyDlvd[CONS]` as the house load for systems with a dedicated consumption CT, falling back to energy balance otherwise. Rejected because there is no reliable way to detect which configuration is present at runtime, and the energy balance gives equivalent results on full-monitoring setups anyway (measured CT values satisfy the same energy balance identity).

### Decision 2: Source grid import/export from EID_NET (`1023410688`), not EID_CONSUMPTION (`704643584`)

**Chosen**: EID_NET is the gateway's dedicated grid-connection meter. Its `actEnergyDlvd` is grid import and `actEnergyRcvd` is grid export by definition. The code already uses EID_NET for real-time `grid_w_now`; using it for cumulative counters is consistent.

**Alternative considered**: Use `actEnergyRcvd` from EID_CONSUMPTION. On net-metering setups these fields are numerically identical to EID_NET. Rejected because EID_CONSUMPTION's `actEnergyRcvd` semantics depend on how the CT is installed; EID_NET is unambiguous.

### Decision 3: Replace `CumulativeReading.consumption_wh` with `grid_import_cum_wh` + `grid_export_cum_wh`

**Chosen**: Rename the field to reflect what it actually measures, and add the missing export counter. This makes the semantics explicit and eliminates the misleading field name that caused the original bug.

**Alternative considered**: Keep `consumption_wh` and add a separate `grid_export_wh` field. Rejected because `consumption_wh` is semantically wrong for what EID_CONSUMPTION delivers; keeping the name would perpetuate the confusion.

### Decision 4: Add a new `config_store` key for `grid_export_cum_wh` persistence

**Chosen**: Add `KEY_GRID_EXPORT_WH = "last_cumulative_grid_export_wh"` alongside the existing production/consumption keys. On first start after upgrade, this key will be absent; `load_persisted_reading` returns `None` in that case (requiring one poll cycle to re-establish the baseline), which is the existing behaviour for a fresh install.

**Alternative considered**: Rename `KEY_CONS_WH` to avoid a stale value being read as grid import. Not needed — the old consumption key (`last_cumulative_consumption_wh`) will simply be ignored once no longer referenced.

## Risks / Trade-offs

- **Accumulated error from energy balance vs. direct measurement**: For systems with a real consumption CT, deriving consumption adds floating-point accumulation across many windows vs. a single direct measurement. In practice the values are within rounding noise of each other since the same meters feed both paths. → Acceptable trade-off given it's the only approach that works for all configurations.

- **EID_NET absent on some gateway firmware versions**: If the gateway omits EID_NET from the response, `grid_import_cum_wh` and `grid_export_cum_wh` default to `0.0`, and `wh_consumed` will equal `wh_produced` (same wrong result as today, but different symptom). → Mitigated by logging a warning when EID_NET is not found in the response.

- **One poll cycle data gap after upgrade**: On first start post-upgrade, `load_persisted_reading` returns `None` (new key not yet set) and the scheduler skips the first window boundary. This is pre-existing behaviour for restarts, not a regression. → Acceptable; no data loss beyond one 15-minute window.

- **Existing stored windows are wrong**: Historical `energy_window` rows where solar was exporting have incorrect `wh_consumed`, `wh_grid_import`, and `wh_grid_export` values. This change does not backfill them. → Documenting this as a known limitation; backfill is a separate concern.

## Migration Plan

1. Deploy updated binary — no DB migration needed (schema unchanged).
2. On first start, `load_persisted_reading` will not find the new `KEY_GRID_EXPORT_WH` key; `last_reading` will be `None` for that one startup cycle.
3. On the next window boundary, the new key is written and all subsequent windows compute correctly.
4. No rollback concern beyond the one-window gap on restart.

## Open Questions

- Should the API response include a metadata flag indicating that historical windows prior to the fix may have incorrect consumption values? Deferred — out of scope for this change.
