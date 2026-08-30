# NEM 2.0 Cost Accuracy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the true-up estimate match SDG&E NEM 2.0 billing by (1) crediting exports at retail minus non-bypassable charges (NBCs) and (2) applying the OpenEI baseline adjustment (`adj`) to energy rates.

**Architecture:** All changes live in the Rust bridge (`enphase-bridge`); the MCP server and dashboard consume the bridge API and inherit the fix with zero changes (response shape is unchanged). The calculator gains a `CostParams` input; the NBC rate is a `[tou]` config value threaded through `AppState`. The OpenEI `sell` field is dropped from the cost model entirely (see Background item 5).

**Tech Stack:** Rust, axum, figment (config), sqlx/SQLite. Tests: cargo unit tests in `tests/unit/`, integration in `tests/integration/`.

**Spec:** Inline — see Background below. No separate spec doc. Adversarially reviewed 2026-08-29 by two independent agents; their findings are folded in.

## Background (the spec)

How SDG&E NEM 2.0 nets, verified 2026-08-29 against CPUC/SDG&E/EnergySage docs:

1. Within a TOU bucket, exports offset imports 1:1 in kWh. In dollar form this means the import rate and the export base rate MUST be the same number for a given bucket (before the NBC haircut). This invariant has a dedicated test in Task 2.
2. Excess credits cross buckets as **dollars at full value**. No "lower rate" for cross-bucket offsets. (Net Surplus Compensation ~$0.03/kWh applies only to annual net exporters at true-up — out of scope; the user is a net importer.)
3. **Gap A — NBCs:** export credits cannot offset non-bypassable charges (~$0.025/kWh for SDG&E residential). Imports already include NBCs in the bundled retail rate; exports must be credited at retail − NBC. Current code credits at full retail → estimate is optimistic.
4. **Gap B — baseline `adj`:** the stored OpenEI rate JSON carries a tier-0 `adj` (e.g. `-0.10543`, the below-baseline credit) which the calculator ignores; it reads only tier-0 `rate`. Applying `adj` to all kWh assumes all usage is below baseline — a deliberate simplification (`ponytail:` comment marks it; the upgrade path is a daily baseline-allowance knob).
5. **Design decision — `sell` is ignored.** OpenEI's `sell` field is inconsistently present: the stored NEM 2.0 schedule in the dev DB (id 1, TOU-D-A) has **no** `sell` on any period, while the checked-in fixtures (`tests/fixtures/sdge_tou_dr2_item.json`, `sdge_tou_dr_coastal_baseline_item.json`) carry `sell == rate` on every period. If `sell` took precedence, a schedule with both `sell` and `adj` would charge imports `rate + adj` but credit exports `sell − NBC`, breaking invariant 1 and overwhelming the NBC correction. Under NEM 2.0 the export credit is retail-based by definition, so the calculator uses `rate + adj` for both sides and drops `sell` entirely. (`ponytail:` comment marks the ceiling — net-billing tariffs with a genuine standalone `sell` rate would need it back.)

Non-goals: baseline tier-2 pricing / daily allowance tracking, NSC for net exporters, minimum-bill charges, changing the API response shape.

Key facts an implementer needs:

- Formula lives in `src/trueup/calculator.rs` (`calculate(schedule, windows)`), called from one production site: `src/api/handlers/trueup.rs:118`, plus ~30 call sites in `tests/unit/calculator_test.rs`.
- `formula_version` on energy windows versions the **window aggregation** formula, NOT the cost formula. The cost calculator runs at request time over stored windows — changing it requires **no recompute and no version bump**. `true_up_estimate` rows are append-only history; no invalidation needed.
- Config loads via figment in `src/config.rs` (`config.toml` merged with `ENPHASE__`-prefixed env vars, `__` splits nesting; the new key is overridable as `ENPHASE__TOU__NBC_RATE_USD_PER_KWH`). `TouConfig` already derives `Deserialize`.
- Test fixture rates (`tests/common/mod.rs::fixture_rate_json`): period 0 super-off-peak $0.15, period 1 off-peak $0.25, period 2 peak $0.40; no `adj`, no `sell`. Timestamp constants like `PEAK_TS`, `SUPER_OP_TS` map to those periods.
- API responses round dollar values via `round2` (`src/api/handlers/trueup.rs:180`, half-away-from-zero: 0.045 → 0.05, 0.0375 → 0.04, 0.1125 → 0.11). Integration assertions compare against **rounded** values.
- `tests/integration/api_auth_test.rs` is **not registered** in `tests/integration.rs` (only `auth_test.rs` is) — it is dead code that references a nonexistent `AppState.api_key` field and never compiles. **Do not edit it**; note it in the PR body as cleanup for a separate change.
- `tests/unit/calculator_test.rs:97` already has `fn make_schedule(rate_json: &str) -> TouRateSchedule` — reuse it; do not add a duplicate schedule-builder helper.

## Global Constraints

- PR targets `dev` (never `main`); branch `feat/nem2-cost-accuracy` created from a freshly synced `dev`.
- API response shape of `/api/trueup/estimate` must not change (MCP + dashboard depend on it).
- No skipped tests; `cargo test` and `cargo clippy -- -D warnings` green before every commit.
- New config key: `[tou] nbc_rate_usd_per_kwh`, default `0.025` (documented, not hidden — appears in `config.example.toml` and **both** README locations: quickstart snippet ~line 84 and the configuration reference table at README.md:224-239). Validated at load: must be in `0.0..=0.20`.
- Period classification (peak/off-peak/super-off-peak ranking) stays keyed on base `rate`, not `rate + adj`. Safe for SDG&E because `adj` is uniform across periods; a code comment records that a per-period `adj` could mislabel buckets (totals stay correct).

---

### Task 1: NBC haircut on export credits (`CostParams`)

**Files:**
- Modify: `src/trueup/calculator.rs` (add `CostParams`, change `calculate` signature, apply haircut at lines ~111-112)
- Modify: `src/api/handlers/trueup.rs:118` (pass `CostParams::default()` — real wiring lands in Task 3)
- Test: `tests/unit/calculator_test.rs`

**Interfaces:**
- Consumes: existing `calculate(&TouRateSchedule, &[EnergyWindow])`.
- Produces: `pub struct CostParams { pub nbc_rate_usd_per_kwh: f64 }` (derives `Debug, Clone, Copy, Default`) and new signature `pub fn calculate(schedule: &TouRateSchedule, windows: &[EnergyWindow], params: CostParams) -> Result<CalculatorResult, AppError>`. Tasks 2 and 3 rely on exactly these names.

- [ ] **Step 1: Write the failing tests** (append to `tests/unit/calculator_test.rs`)

```rust
#[test]
fn test_nbc_reduces_export_credit() {
    let schedule = fixture_schedule();
    let windows = vec![make_window(SUPER_OP_TS, 0.0, 1000.0)];
    let params = calculator::CostParams {
        nbc_rate_usd_per_kwh: 0.025,
    };
    let result = calculator::calculate(&schedule, &windows, params).unwrap();
    // 1 kWh × ($0.15 − $0.025)
    assert!((result.super_off_peak.export_credit_usd - 0.125).abs() < 1e-6);
    // imports untouched by NBC
    assert_eq!(result.super_off_peak.import_cost_usd, 0.0);
}

#[test]
fn test_nbc_larger_than_rate_floors_credit_at_zero() {
    let schedule = fixture_schedule();
    let windows = vec![make_window(SUPER_OP_TS, 0.0, 1000.0)];
    let params = calculator::CostParams {
        nbc_rate_usd_per_kwh: 0.20,
    };
    let result = calculator::calculate(&schedule, &windows, params).unwrap();
    assert_eq!(result.super_off_peak.export_credit_usd, 0.0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test unit nbc`
Expected: compile FAIL — `CostParams` not found / `calculate` takes 2 arguments.

- [ ] **Step 3: Implement in `src/trueup/calculator.rs`**

Add above `calculate`:

```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct CostParams {
    /// NEM 2.0: export credits cannot offset non-bypassable charges, so the
    /// export credit rate is retail minus this. Imports are untouched — the
    /// bundled retail rate already includes NBCs.
    pub nbc_rate_usd_per_kwh: f64,
}
```

Change the signature:

```rust
pub fn calculate(
    schedule: &TouRateSchedule,
    windows: &[EnergyWindow],
    params: CostParams,
) -> Result<CalculatorResult, AppError> {
```

Replace the export credit line (`acc.export_credit_usd += export_kwh * rates.sell_rate;`) with:

```rust
let export_rate = (rates.sell_rate - params.nbc_rate_usd_per_kwh).max(0.0);
acc.export_credit_usd += export_kwh * export_rate;
```

Fix all call sites so everything compiles:
- `src/api/handlers/trueup.rs:118` → `calculator::calculate(&schedule, &windows, calculator::CostParams::default())`
- `tests/unit/calculator_test.rs` has **~30 calls** with varying second args (`&windows`, `&[make_window(...)]`, `&[]`, `&bad_schedule` variants) — add `, calculator::CostParams::default()` as the third argument to every one. Run `cargo check --tests` and let the compiler enumerate them; do not rely on a text search for one exact pattern. Default NBC = 0.0, so all existing assertions still hold.

- [ ] **Step 4: Run the full suite**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS, including both new tests.

- [ ] **Step 5: Commit**

```bash
git add src/trueup/calculator.rs src/api/handlers/trueup.rs tests/unit/calculator_test.rs
git commit -m "feat(trueup): subtract non-bypassable charges from export credits"
```

---

### Task 2: Apply OpenEI baseline adjustment (`adj`); drop `sell` from the model

**Files:**
- Modify: `src/trueup/calculator.rs` (`PeriodRate`, `parse_period_rates`, cost accumulation, ranking comment)
- Test: `tests/unit/calculator_test.rs`

**Interfaces:**
- Consumes: `CostParams` and 3-arg `calculate` from Task 1.
- Produces: `PeriodRate` becomes `struct PeriodRate { rate: f64, adj: f64 }` (private — nothing outside calculator.rs touches it). Effective import rate = `rate + adj`; export base rate = the same `rate + adj` (invariant 1). `sell_rate` and the `tou_sell_rate_missing` warn are deleted.

- [ ] **Step 1: Write the failing tests** (append to `tests/unit/calculator_test.rs`)

```rust
fn rate_json_with_structure(structure: &str) -> String {
    let weekday_row = "[0,0,0,0,0,0,1,1,1,1,1,1,1,1,1,1,2,2,2,2,2,1,1,1]";
    let weekend_row = "[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]";
    let months = vec![weekday_row; 12].join(",");
    let weekend_months = vec![weekend_row; 12].join(",");
    format!(
        r#"{{"energyweekdayschedule":[{months}],"energyweekendschedule":[{weekend_months}],"energyratestructure":{structure}}}"#
    )
}

#[test]
fn test_adj_applied_to_import_rate() {
    // peak (period 2): rate 0.40 + adj −0.10 → effective 0.30
    let json = rate_json_with_structure(
        r#"[[{"rate":0.15}],[{"rate":0.25}],[{"rate":0.40,"adj":-0.10}]]"#,
    );
    let schedule = make_schedule(&json);
    let windows = vec![make_window(PEAK_TS, 1000.0, 0.0)];
    let result =
        calculator::calculate(&schedule, &windows, calculator::CostParams::default()).unwrap();
    assert!((result.peak.import_cost_usd - 0.30).abs() < 1e-6);
}

#[test]
fn test_adj_applied_to_export_rate() {
    let json = rate_json_with_structure(
        r#"[[{"rate":0.15}],[{"rate":0.25}],[{"rate":0.40,"adj":-0.10}]]"#,
    );
    let schedule = make_schedule(&json);
    let windows = vec![make_window(PEAK_TS, 0.0, 1000.0)];
    let result =
        calculator::calculate(&schedule, &windows, calculator::CostParams::default()).unwrap();
    assert!((result.peak.export_credit_usd - 0.30).abs() < 1e-6);
}

#[test]
fn test_sell_field_is_ignored() {
    // sell present (OpenEI mirror convention or otherwise) must NOT override
    // the retail-based credit: expect rate + adj = 0.30, not sell = 0.05
    let json = rate_json_with_structure(
        r#"[[{"rate":0.15}],[{"rate":0.25}],[{"rate":0.40,"adj":-0.10,"sell":0.05}]]"#,
    );
    let schedule = make_schedule(&json);
    let windows = vec![make_window(PEAK_TS, 0.0, 1000.0)];
    let result =
        calculator::calculate(&schedule, &windows, calculator::CostParams::default()).unwrap();
    assert!((result.peak.export_credit_usd - 0.30).abs() < 1e-6);
}

#[test]
fn test_within_bucket_netting_is_one_to_one_before_nbc() {
    // Background invariant 1: with NBC 0, importing and exporting the same
    // kWh in the same bucket nets to exactly zero — even when adj is present.
    let json = rate_json_with_structure(
        r#"[[{"rate":0.15}],[{"rate":0.25}],[{"rate":0.40,"adj":-0.10,"sell":0.05}]]"#,
    );
    let schedule = make_schedule(&json);
    let windows = vec![make_window(PEAK_TS, 1000.0, 1000.0)];
    let result =
        calculator::calculate(&schedule, &windows, calculator::CostParams::default()).unwrap();
    assert!(result.net_cost_usd.abs() < 1e-9);
}

#[test]
fn test_missing_adj_defaults_to_zero() {
    // the shared fixture has no adj — same numbers as before this feature
    let schedule = fixture_schedule();
    let windows = vec![make_window(PEAK_TS, 500.0, 0.0)];
    let result =
        calculator::calculate(&schedule, &windows, calculator::CostParams::default()).unwrap();
    assert!((result.peak.import_cost_usd - 0.20).abs() < 1e-6);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test unit adj && cargo test --test unit sell`
Expected: `test_adj_applied_to_import_rate`, `test_adj_applied_to_export_rate`, `test_sell_field_is_ignored`, and `test_within_bucket_netting_is_one_to_one_before_nbc` FAIL (adj ignored and/or sell honored). `test_missing_adj_defaults_to_zero` may already pass — that's fine.

- [ ] **Step 3: Implement in `src/trueup/calculator.rs`**

Change `PeriodRate`:

```rust
struct PeriodRate {
    rate: f64,
    adj: f64,
}
```

In `parse_period_rates`, replace lines ~183-189 — the whole block from `let sell_rate = if let Some(s) ...` through the existing `Ok(PeriodRate { rate, sell_rate })` — with:

```rust
// ponytail: adj applied to ALL kWh — assumes everything is below the
// baseline allowance. Upgrade path: a daily baseline-allowance knob
// that switches to tier-2 rates past the threshold.
// ponytail: OpenEI "sell" is deliberately ignored — NEM 2.0 credits at
// retail; net-billing tariffs with a real standalone sell rate need it back.
let adj = tier["adj"].as_f64().unwrap_or(0.0);
Ok(PeriodRate { rate, adj })
```

(The `tou_sell_rate_missing` warn is deleted — a missing `sell` is the normal case, not an anomaly.)

In the accumulation loop, replace the import/export cost lines (including Task 1's export line):

```rust
let effective_rate = rates.rate + rates.adj;
let export_rate = (effective_rate - params.nbc_rate_usd_per_kwh).max(0.0);
acc.import_cost_usd += import_kwh * effective_rate;
acc.export_credit_usd += export_kwh * export_rate;
```

In `build_per_month_maps`, keep ranking by `pr.rate` but add above the sort (line ~241):

```rust
// Ranking uses base rate, not rate + adj: adj is uniform across periods on
// SDG&E schedules. A per-period adj could mislabel peak/off-peak buckets
// (totals would stay correct).
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS. (The shared fixture has no adj/sell, so all pre-existing assertions are unaffected.)

- [ ] **Step 5: Commit**

```bash
git add src/trueup/calculator.rs tests/unit/calculator_test.rs
git commit -m "feat(trueup): apply baseline adjustment (adj) and drop OpenEI sell from cost model"
```

---

### Task 3: Configurable NBC rate wired through config → AppState → handler

**Files:**
- Modify: `src/config.rs` (`TouConfig` field + default fn + load-time validation)
- Modify: `src/api/server.rs:10-19` (`AppState` field)
- Modify: `src/main.rs:53-62` (AppState construction)
- Modify: `src/api/handlers/trueup.rs:118` (pass real value)
- Modify: `config.example.toml`; `README.md` quickstart `[tou]` snippet (~line 84) **and** configuration reference table (README.md:224-239)
- Modify: every `AppState {` literal in **registered** integration tests — grep `AppState {` under `tests/integration/` and cross-check `tests/integration.rs`; expected files: `api_energy_test.rs`, `api_phases_test.rs`, `trueup_test.rs`, `tou_refresh_test.rs`, `api_inverter_test.rs`, `api_power_test.rs`, `health_test.rs`. **Skip `api_auth_test.rs`** (unregistered dead file — see Background).
- Test: `tests/unit/config_test.rs` (new, registered in `tests/unit.rs`), `tests/integration/trueup_test.rs`

**Interfaces:**
- Consumes: `CostParams` from Task 1.
- Produces: `TouConfig.nbc_rate_usd_per_kwh: f64` (serde default 0.025, validated `0.0..=0.20`), `AppState.tou_nbc_rate_usd_per_kwh: f64`, `Config::validate()`.

- [ ] **Step 1: Write the failing config tests** — create `tests/unit/config_test.rs` and register it in `tests/unit.rs` the same way the other unit modules are registered:

```rust
use enphase_bridge::config::Config;
use figment::{
    Figment,
    providers::{Format, Toml},
};

const BASE_TOML: &str = r#"
[gateway]
host = "envoy.local"
token = "t"

[polling]
interval_secs = 60

[api]
host = "127.0.0.1"
port = 8080

[storage]
db_path = ":memory:"

[tou]
openei_api_key = "k"
utility_eia_id = 17609
rate_label = "TOU-DR-1"

[arrays]
"#;

fn parse(toml: &str) -> Config {
    Figment::new()
        .merge(Toml::string(toml))
        .extract()
        .expect("config should parse")
}

#[test]
fn test_nbc_rate_defaults_when_absent() {
    let cfg = parse(BASE_TOML);
    assert!((cfg.tou.nbc_rate_usd_per_kwh - 0.025).abs() < 1e-9);
    assert!(cfg.validate().is_ok());
}

#[test]
fn test_nbc_rate_overridable() {
    let toml = BASE_TOML.replace(
        "rate_label = \"TOU-DR-1\"",
        "rate_label = \"TOU-DR-1\"\nnbc_rate_usd_per_kwh = 0.031",
    );
    let cfg = parse(&toml);
    assert!((cfg.tou.nbc_rate_usd_per_kwh - 0.031).abs() < 1e-9);
}

#[test]
fn test_negative_nbc_rate_rejected() {
    let toml = BASE_TOML.replace(
        "rate_label = \"TOU-DR-1\"",
        "rate_label = \"TOU-DR-1\"\nnbc_rate_usd_per_kwh = -0.01",
    );
    let cfg = parse(&toml);
    assert!(cfg.validate().is_err());
}
```

(If `Config` has required fields this TOML misses, extend `BASE_TOML` until extraction succeeds — the assertion targets stay the same.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test unit config`
Expected: compile FAIL — `nbc_rate_usd_per_kwh` not a field of `TouConfig`, `validate` not found.

- [ ] **Step 3: Implement the wiring**

`src/config.rs`:

```rust
pub struct TouConfig {
    pub openei_api_key: String,
    pub utility_eia_id: u32,
    pub rate_label: String,
    /// USD/kWh haircut on export credits (NEM 2.0 non-bypassable charges).
    /// ponytail: single flat value — NBCs change yearly; update via config.
    #[serde(default = "default_nbc_rate")]
    pub nbc_rate_usd_per_kwh: f64,
}

fn default_nbc_rate() -> f64 {
    0.025 // SDG&E residential NBCs, ~2026
}
```

In `impl Config`, add validation and call it from `load` (no silent acceptance of nonsense values):

```rust
pub fn load() -> anyhow::Result<Self> {
    let cfg: Self = Figment::new()
        .merge(Toml::file("config.toml"))
        .merge(Env::prefixed("ENPHASE__").split("__"))
        .extract()?;
    cfg.validate()?;
    Ok(cfg)
}

pub fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
        (0.0..=0.20).contains(&self.tou.nbc_rate_usd_per_kwh),
        "tou.nbc_rate_usd_per_kwh must be in 0.0..=0.20 USD/kWh, got {}",
        self.tou.nbc_rate_usd_per_kwh
    );
    Ok(())
}
```

`src/api/server.rs` — add to `AppState`:

```rust
pub tou_nbc_rate_usd_per_kwh: f64,
```

`src/main.rs` — in the `AppState` literal (lines 53-62):

```rust
tou_nbc_rate_usd_per_kwh: config.tou.nbc_rate_usd_per_kwh,
```

`src/api/handlers/trueup.rs:118`:

```rust
let result = calculator::calculate(
    &schedule,
    &windows,
    calculator::CostParams {
        nbc_rate_usd_per_kwh: state.tou_nbc_rate_usd_per_kwh,
    },
)?;
```

Every `AppState {` literal in the **registered** integration test files listed above gains:

```rust
tou_nbc_rate_usd_per_kwh: 0.0,
```

(0.0 keeps all existing dollar assertions valid. Run `cargo check --tests` to catch any literal the grep missed.)

`config.example.toml`, `[tou]` section — add:

```toml
# NEM 2.0 non-bypassable charges (USD/kWh), subtracted from export credits.
# SDG&E residential default; update when the utility revises NBCs.
nbc_rate_usd_per_kwh = 0.025
```

`README.md`: add the same key to the quickstart `[tou]` snippet (~line 84) **and** a row in the configuration reference table (README.md:224-239, alongside the other `tou.*` rows).

- [ ] **Step 4: Add one integration test proving the wiring** (append to `tests/integration/trueup_test.rs`). The sibling `test_estimate_returns_correct_net_cost` seeds **three** windows — peak 500 Wh import, super-off-peak 300 Wh export, off-peak 200 Wh import + 500 Wh export. Copy its body, change the state to carry `tou_nbc_rate_usd_per_kwh: 0.025` (this test builds `AppState` inline rather than via `make_state`, or extends `make_state` with an NBC parameter — implementer's choice, smallest diff wins), and assert the NBC-adjusted, **round2-rounded** values:

```rust
#[tokio::test]
async fn test_estimate_applies_configured_nbc_rate() {
    // Same seeding as test_estimate_returns_correct_net_cost:
    //   peak:      0.5 kWh import           → +0.5×0.40          = +0.2000
    //   super-op:  0.3 kWh export           → −0.3×(0.15−0.025)  = −0.0375
    //   off-peak:  0.2 import + 0.5 export  → +0.2×0.25 − 0.5×(0.25−0.025) = +0.05 − 0.1125
    // net = 0.2000 + 0.0500 − 0.0375 − 0.1125 = 0.1000
    // Response values are round2-rounded: net 0.10, super-op credit 0.04, off-peak credit 0.11.
    //
    // ... [seed pool + schedule + 3 windows exactly like the sibling test] ...
    //
    // assert!((j["net_cost_usd"].as_f64().unwrap() - 0.10).abs() < 1e-6);
    // assert!((j["breakdown"]["super_off_peak"]["export_credit_usd"].as_f64().unwrap() - 0.04).abs() < 1e-6);
    // assert!((j["breakdown"]["off_peak"]["export_credit_usd"].as_f64().unwrap() - 0.11).abs() < 1e-6);
    // assert!((j["breakdown"]["peak"]["import_cost_usd"].as_f64().unwrap() - 0.20).abs() < 1e-6);
}
```

- [ ] **Step 5: Run the full suite**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/api/server.rs src/main.rs src/api/handlers/trueup.rs \
  config.example.toml README.md tests/unit/config_test.rs tests/unit.rs tests/integration
git commit -m "feat(config): configurable NEM 2.0 NBC rate wired into true-up estimates"
```

---

## PR

- Branch `feat/nem2-cost-accuracy` from synced `dev`; PR targets `dev`.
- PR body: link the NEM 2.0 facts from Background, note the deliberate ceilings (flat NBC value; adj applied to all kWh; `sell` ignored) and their upgrade paths, state that MCP + dashboard need no changes, and flag `tests/integration/api_auth_test.rs` as unregistered dead code for a separate cleanup PR.

## Downstream (explicitly no-op)

- `enphase-bridge-mcp`: `cost_tools.py` proxies fields already present — no change.
- `enphase-bridge-dashboard`: `trueupBuckets.ts` aggregates API responses — no change.
- No `formula_version` bump, no window recompute: cost math runs at request time; window aggregation is untouched.
