# North Star & Feature Completeness Checklist: Optional API Key Authentication

**Purpose**: Validate that all requirements honour the North Star ("easy to use, sensible defaults, depth is opt-in") and that every design decision reached through the speckit → clarify → Opus Council → plan workflow is captured in the spec and plan.
**Created**: 2026-04-26
**Feature**: [spec.md](../spec.md) | [plan.md](../plan.md)

---

## North Star Alignment

- [X] CHK001 - Is "auth disabled by default" stated as a non-negotiable default in both the spec and plan, with no pathway to ship it enabled by default? [Completeness, Spec §FR-001]
- [X] CHK002 - Is the rationale for every requirement explicitly tied to a customer ask or safety need, with no requirement that exists purely for engineering elegance? [Clarity, Spec §FR-001–013]
- [X] CHK003 - Is the single-toggle enable path (`require_auth = true`) the only required user action to enable protection — are there any requirements that demand additional setup steps? [Measurability, Spec §SC-001]
- [X] CHK004 - Is the auto-generation path (no `api_key` in config) specified as the zero-friction route, so a user can enable auth with exactly one config line? [Completeness, Spec §US3]
- [X] CHK005 - Is SC-003 ("existing deployments experience zero behavioural change") stated with the word "upgrading" so it is unambiguous as an upgrade compatibility guarantee? [Clarity, Spec §SC-003]
- [X] CHK006 - Is the decision to remove the character-diversity check (dropped from FR-004) documented — and is the rationale ("complexity not asked for by the customer") traceable to the North Star? [Consistency, Spec §FR-004, Plan §Key Design Decisions]
- [X] CHK007 - Are all security hardening requirements (constant-time comparison, zeroize, TLS warning) invisible to end-users at runtime — i.e., do none of them require the user to take an additional action? [Coverage, Spec §FR-004, FR-012, FR-013]
- [X] CHK008 - Is the rate-limiting out-of-scope decision documented with a customer-first rationale (delegate to reverse proxy / OS, not the daemon's responsibility)? [Consistency, Spec §Assumptions]

---

## Feature Requirements Completeness (Design Decisions Captured)

- [X] CHK009 - Is constant-time comparison explicitly required by name in FR-004, not left as an implementation detail for the developer to discover or skip? [Completeness, Spec §FR-004]
- [X] CHK010 - Is router-scoped middleware (as opposed to per-handler checks) mandated in FR-003 with an explicit prohibition on per-handler enforcement? [Clarity, Spec §FR-003]
- [X] CHK011 - Is the `API_KEY_GENERATED` literal tag required in FR-006 as a machine-findable string — not just "prominent log line"? [Clarity, Spec §FR-006]
- [X] CHK012 - Does FR-006 require both the key line AND an immediately following ephemeral-key warning line as two distinct log events, not a single combined message? [Completeness, Spec §FR-006]
- [X] CHK013 - Is the startup confirmation log for user-configured keys (FR-011) specified with a concrete required string ("API authentication enabled") and the key-length field? [Clarity, Spec §FR-011]
- [X] CHK014 - Is the TLS warning requirement (FR-012) tied to a specific trigger condition (non-loopback bind address), not just "when not using a proxy"? [Clarity, Spec §FR-012]
- [X] CHK015 - Is zeroize-on-shutdown (FR-013) specified as a MUST, not a SHOULD, and is it framed in terms of observable behaviour (key overwritten in memory before deallocation)? [Clarity, Spec §FR-013]
- [X] CHK016 - Is the identical-401-body requirement (FR-010) specified with no-oracle-leakage rationale — i.e., does it explicitly state that all failure modes return the same body? [Completeness, Spec §FR-010]
- [X] CHK017 - Is the health endpoint exemption (FR-007) constrained by an enumerated path (`/api/health`) rather than an open-ended category ("health-check routes")? [Clarity, Spec §FR-007]
- [X] CHK018 - Does FR-007 explicitly prohibit the exempt endpoint from returning version, config, or gateway state — ensuring health stays "liveness only"? [Completeness, Spec §FR-007]
- [X] CHK019 - Is the distinct exit code (exit 2 for config error vs exit 1 for token error) captured in the plan's startup flow — and is it traceable from the spec's edge cases? [Consistency, Spec §Edge Cases, Plan §Startup Flow]
- [X] CHK020 - Is the whitespace-only key edge case (treated as absent → auto-generate) documented in both the spec edge cases AND the data-model validation rules? [Consistency, Spec §Edge Cases, data-model.md §AuthConfiguration]
- [X] CHK021 - Is the crash-restart-loop behaviour under `restart: unless-stopped` called out as expected and intentional — not as an error condition — in both spec and plan? [Clarity, Spec §Edge Cases, Plan §Startup Flow]

---

## Workflow Completeness (Speckit Workflow Artifacts)

- [X] CHK022 - Are all three Opus Council recommendations (CRITICAL: entropy, timing, log survivability) traceable to changes in the spec — can each finding be mapped to a specific FR that addresses it? [Traceability, Spec §FR-004, FR-005, FR-006]
- [X] CHK023 - Is the `/api/health` vs `/health` path discrepancy (spec assumed `/health`, code has `/api/health`) resolved in the plan with a documented decision to keep `/api/health` for backward compatibility? [Consistency, Plan §Health Endpoint Path]
- [X] CHK024 - Does the plan document the parallel subagent execution strategy (Test-A, Test-B, Impl-A, Impl-B) with explicit dependency ordering — specifically that Impl-B waits on Impl-A? [Completeness, Plan §Parallel Execution Strategy]
- [X] CHK025 - Are all three new Cargo dependencies (`subtle`, `zeroize`, `rand`) documented in the plan with their exact versions and the rationale for each? [Completeness, research.md]
- [X] CHK026 - Does the API contract (`contracts/api.md`) cover all startup signal log formats with exact field names (`event`, `api_key`, `key_len`, `message`) so they are testable without reading source code? [Clarity, contracts/api.md §Startup Signals]
- [X] CHK027 - Does the quickstart cover all five scenarios discussed (default access, auto-gen key, static key, disable auth, short-key failure) with concrete shell commands a non-developer can follow? [Coverage, quickstart.md]
- [X] CHK028 - Is the `Zeroizing<String>` in `main` / plain `String` in `AppState` split decision documented in the plan with a rationale (Clone compatibility) rather than left implicit? [Clarity, Plan §Key Design Decisions]

---

## Acceptance Criteria Measurability

- [X] CHK029 - Can SC-002 ("obtain a working key within 10 seconds") be objectively verified without subjective interpretation — e.g., is there a defined test method (grep log for `API_KEY_GENERATED`, use key in request)? [Measurability, Spec §SC-002]
- [X] CHK030 - Can SC-004 ("latency bounded and independent of key correctness") be verified without access to the source code — e.g., is there a stated test method (time correct vs incorrect key responses over N requests)? [Measurability, Spec §SC-004]
- [X] CHK031 - Is SC-005 ("100% of `/api/*` routes protected") verifiable by enumerating routes — does the contract list every protected route explicitly so coverage can be audited? [Measurability, Spec §SC-005, contracts/api.md]

---

## Notes

- Check items off as completed: `[x]`
- Add inline findings or spec-update references when an item reveals a gap
- Items marked `[Gap]` indicate a missing requirement; update the spec before proceeding to `/speckit-tasks`
- Items marked `[Consistency]` indicate a cross-document alignment check; verify both sources match
