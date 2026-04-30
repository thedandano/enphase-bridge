## 1. Fixture

- [x] 1.1 Create `tests/fixtures/sdge_tou_dr_coastal_baseline_item.json` — 6-period seasonal rate with periods {0,1,2} for months 5–9 and {3,4,5} for months 0–4 and 10–11, rates ordered so global ranking ≠ per-month ranking

## 2. Calculator — core fix

- [x] 2.1 Remove `build_period_map` and replace with `build_per_month_maps(weekday_sched, weekend_sched, period_rates) -> Result<[HashMap<usize, TouPeriod>; 12], AppError>` — builds active-period union per month using `BTreeSet`, ranks within each set, returns 12 maps
- [x] 2.2 Emit `tracing::info!(event="tou_period_map_built", month, active_periods, peak_idx, super_off_peak_idx)` for each month inside `build_per_month_maps`
- [x] 2.3 Emit `tracing::warn!(event="tou_month_degenerate", month, active_count)` when a month's active set has fewer than 2 distinct indices
- [x] 2.4 Remove the global `tou_period_count_unexpected` warning
- [x] 2.5 Replace `period_map.get(&period_idx).copied().unwrap_or(TouPeriod::OffPeak)` with explicit `Err(TouError::ParseError(format!("period_idx={period_idx} not in month={month} map")))` 
- [x] 2.6 Update `calculate()` to call `build_per_month_maps` upfront, then look up `month_maps[month]` per window

## 3. Unit tests

- [x] 3.1 6-period seasonal, winter month (January, hour 16) → Peak; hour 0 → Super Off-Peak; hour 8 → Off-Peak
- [x] 3.2 6-period seasonal, summer month (July, hour 16) → Peak; hour 0 → Super Off-Peak; hour 8 → Off-Peak
- [x] 3.3 6-period, cross-season: same hour in January and July both classify as Peak within their respective groups
- [x] 3.4 Weekend-only period: month where an index appears only in the weekend schedule row is included in the active set and correctly classified
- [x] 3.5 n=1 active period in a month: single index classifies as Peak; degenerate warning emitted
- [x] 3.6 n=2 active periods: highest → Peak, other → Off-Peak, no Super Off-Peak
- [x] 3.7 TOU-DR-2 regression: existing fixture produces same Peak/Off-Peak/Super Off-Peak results as before (add OffPeak and SuperOffPeak window assertions alongside existing Peak test)
- [x] 3.8 Invalid period index (period in schedule row not in period_rates) → explicit `TouError::ParseError`, not Off-Peak fallback

## 4. Integration tests

- [x] 4.1 Add integration test in `tests/integration/trueup_test.rs` seeded with the 6-period coastal baseline fixture — assert API response breakdown shows Peak and Super Off-Peak in a summer month window

## 5. Config update

- [x] 5.1 Update `config.toml` `rate_label` from `"TOU-DR Coastal Baseline Region"` to `"TOU-DR-2"`
