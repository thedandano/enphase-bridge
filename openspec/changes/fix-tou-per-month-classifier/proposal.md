## Why

`build_period_map` ranks all TOU periods globally by rate, which correctly handles 3-period rates but silently misclassifies every winter hour as Off-Peak on 6-period seasonal rates (like SDG&E TOU-DR Coastal Baseline). The configured rate has two implicit seasonal groups — winters use indices {3,4,5}, summers use {0,1,2} — and since all three winter periods fall between the summer Peak and Super Off-Peak globally, the classifier never produces Peak or Super Off-Peak in winter months, yielding wrong true-up estimates without any error signal.

## What Changes

- `build_period_map` is replaced with a per-month variant that ranks within the set of period indices actually used in each month's schedule
- Active period set for each month is derived from the union of weekday and weekend schedule rows (so weekend-only periods are not missed when classifying weekday windows)
- `unwrap_or(TouPeriod::OffPeak)` implicit fallback is replaced with an explicit `TouError::ParseError`
- `tou_period_count_unexpected` global warning is replaced with a per-month degenerate warning (fewer than 2 distinct active periods in a month)
- A structured `tou_period_map_built` info event is emitted per month at calculate time
- A new 6-period seasonal fixture is added alongside the existing TOU-DR-2 fixture
- Config `rate_label` updated to `TOU-DR-2` (3-period, matches the user's actual tariff and simplest correct path)

## Capabilities

### New Capabilities

- `tou-per-month-classification`: TOU period ranking scoped per calendar month rather than globally across the full rate structure, enabling correct Peak/Off-Peak/Super Off-Peak classification for any OpenEI rate regardless of the number of period groups or seasonal splits

### Modified Capabilities

- `tou-observability`: Structured log events for TOU classification expand from aggregate (`trueup_calc_done`) to include per-month period map inference (`tou_period_map_built`) and explicit errors on invariant violations

## Impact

- `src/trueup/calculator.rs` — `build_period_map` signature and call sites
- `tests/unit/calculator_test.rs` — new test cases for 6-period seasonal rates, edge cases (n=1, n=2 active per month), weekend/weekday union
- `tests/fixtures/` — new `sdge_tou_dr_coastal_baseline_item.json` fixture
- `tests/integration/trueup_test.rs` — integration test seeded with 6-period fixture
- `config.toml` — `rate_label` updated to `TOU-DR-2`
- No API surface changes (`CalculatorResult` shape unchanged)
