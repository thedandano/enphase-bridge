## Context

`src/trueup/calculator.rs` computes NEM true-up estimates by classifying each 15-minute energy window into a TOU period (Peak / Off-Peak / Super Off-Peak) and accumulating kWh and cost per bucket.

The current `build_period_map` function:
1. Takes all periods from `energyratestructure` (can be 3–6+)
2. Sorts globally by rate descending
3. Assigns rank 0 → Peak, rank n-1 → Super Off-Peak (if n ≥ 3), rest → Off-Peak
4. Returns one `HashMap<usize, TouPeriod>` used for all 12 months

This breaks for any rate with multiple seasonal period groups (e.g., SDG&E TOU-DR Coastal Baseline: periods {0,1,2} for summer, {3,4,5} for winter). All three winter periods rank in the middle globally, so every winter hour classifies as Off-Peak regardless of the actual hour.

## Goals / Non-Goals

**Goals:**
- Classify each window's TOU period based on the rate tiers active in *that month's schedule*, not globally
- Eliminate the implicit `unwrap_or(TouPeriod::OffPeak)` fallback — make invariant violations explicit errors
- Emit a per-month structured log event so operators can verify the classifier's inference
- Keep TOU-DR-2 (3-period, same active set every month) behavior identical to today
- All correctness validated by unit tests including the 6-period seasonal fixture

**Non-Goals:**
- Multi-tier consumption rates (tiers beyond `tiers.first()` remain out of scope)
- Holiday schedule support (OpenEI `energyholidayschedule` field ignored)
- Changes to the API surface or `CalculatorResult` shape

## Decisions

### D1: Per-month period maps pre-computed upfront, not lazily per-window

Build an array `[HashMap<usize, TouPeriod>; 12]` at the top of `calculate()` before the window loop. Each map covers exactly the period indices used in that month.

**Why:** Pre-computation is O(12 × n log n) once vs. O(windows × n log n) per-window. More importantly, it makes the map-build logic separable from window iteration — easier to test and reason about. Lazy memoization adds branching inside the hot loop.

### D2: Active period set = union of weekday AND weekend schedule rows for the month

For month `m`, collect distinct period indices from both `weekday_sched[m]` and `weekend_sched[m]`, then rank within that union.

**Why:** A period that appears only on weekends would be absent from a weekday-only derivation. When a weekday window arrives for that month, the map lookup would fail — hitting the now-explicit error path. The union ensures every period referenced in any window for that month is present in the map.

### D3: Use `BTreeSet` for collecting distinct active indices

Collect per-month active indices into a `BTreeSet<usize>` (sorted, deterministic iteration), convert to `Vec<(usize, f64)>` with rates, then stable-sort by rate descending.

**Why:** `HashSet` has randomized iteration order, which breaks tie-breaking determinism. `BTreeSet` gives sorted iteration, matching current behavior (stable sort by rate, original index order breaks ties).

### D4: Replace `unwrap_or(TouPeriod::OffPeak)` with explicit error

`period_map[month].get(&period_idx)` returning `None` is an invariant violation (a period index appeared in the schedule but was not included in the active set for that month). Return `Err(TouError::ParseError(format!("period_idx={period_idx} not in month={month} map")))` — maps to HTTP 502.

**Why:** The project invariant is "no implicit fallbacks." An Off-Peak default hides data quality issues. HTTP 502 is already the contract for `TouError::ParseError` and signals "upstream data problem."

### D5: Replace global `tou_period_count_unexpected` with per-month degenerate warning

Remove the global `if !(2..=4).contains(&n)` check. Instead, after building each month's active set, warn if fewer than 2 distinct indices are active (a single-period month means every hour is "Peak" — likely a data anomaly).

**Why:** The global warning fires on every request for a 6-period seasonal rate (the now-normal case) and carries no actionable signal. A per-month degenerate check catches the genuinely unexpected case.

### D6: Emit `tou_period_map_built` info event per month

After building each month's map, emit:
```
event="tou_period_map_built", month=M, active_periods=[...], peak_idx=X, super_off_peak_idx=Y
```

**Why:** Without this, a future regression (wrong active set, wrong rank) is invisible. `trueup_calc_done` only shows aggregate kWh and gives no way to reconstruct what the classifier inferred for each month.

## Risks / Trade-offs

- **Single-period months classify everything as Peak** → Degenerate warning (D5) surfaces this; not a silent failure, but semantically odd. Document as known behavior.
- **Rates where the same period index appears in multiple month groups** → Per-month handles this correctly (each month's map is independent). No risk.
- **The per-month "Peak" label means different absolute rates across seasons** → Cost totals remain correct (actual `rates.rate` applied per window at line 115, independent of label). kWh bucket totals are rate-tier categorizations, not fixed price levels. Acceptable: this matches how real NEM true-up bills work.
- **Increased up-front compute** → 12 map builds instead of 1. Negligible for any realistic period count.

## Migration Plan

1. Update config `rate_label` to `TOU-DR-2` — immediately correct results for the user's actual tariff
2. Deploy updated calculator — existing data re-classified on next API call (estimates are computed on demand, not cached across restarts)
3. No DB migration needed — `true_up_estimate` rows are point-in-time snapshots; old rows with wrong classification are not back-filled (acceptable: they're historical estimates, not billing records)
4. Rollback: revert `calculator.rs` — no schema changes to undo
