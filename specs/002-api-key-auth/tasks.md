# Tasks: Optional API Key Authentication

**Feature**: `002-api-key-auth` | **Date**: 2026-04-26
**Input**: `specs/002-api-key-auth/` (spec.md, plan.md, research.md, data-model.md, contracts/api.md, quickstart.md)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[US#]**: User story label — US1 (P1), US2 (P2), US3 (P3)
- All tasks include exact file paths per the implementation plan

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Add new Cargo dependencies and create the middleware module skeleton before any user story work begins.

- [X] T001 Add `subtle = "2.6"`, `zeroize = { version = "1.8", features = ["derive"] }`, and `rand = "0.9"` to `[dependencies]` in `Cargo.toml`
- [X] T002 [P] Create `src/api/middleware/mod.rs` declaring `pub mod api_key;` AND create an empty `src/api/middleware/api_key.rs` stub (prevents `cargo check` failure between T002 and T009 when the module is declared but not yet implemented)
- [X] T003 [P] Add `pub mod middleware;` to `src/api/mod.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Extend the config struct and AppState so the compiler surface is stable before tests or middleware are written.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete — both test files and the middleware depend on these struct shapes.

- [X] T004 Extend `ApiConfig` in `src/config.rs` with `#[serde(default)] pub require_auth: bool` and `pub api_key: Option<String>`
- [X] T005 Add `pub api_key: Option<String>` field to `AppState` in `src/api/server.rs` (plain `String` clone, not `Zeroizing`; `Zeroizing<String>` is held in `main.rs` only)

**Checkpoint**: Struct changes compile. All downstream files that construct `AppState` must be updated to include the new field (pass `None` as a placeholder until startup logic is wired in Phase 4/5).

---

## Phase 3: User Story 1 — Zero-Friction Default Access (Priority: P1) 🎯 MVP

**Goal**: Existing deployments with no config changes continue to work exactly as before. Auth is off by default; no `Authorization` header is required on any `/api/*` route when `require_auth = false`.

**Independent Test**: Run service with default config (no `require_auth`); `curl http://localhost:8080/api/energy/windows` returns 200 with no Authorization header.

### Tests for User Story 1 (TDD — write first, verify they FAIL before implementation)

> **⚠️ Write these tests first. Confirm they compile but FAIL before proceeding to implementation.**

- [X] T006 [P] [US1] Write unit test `test_middleware_passthrough_when_disabled` in `tests/unit/api_key_test.rs` — middleware with `api_key = None` in state calls `next.run(request)` without inspecting the Authorization header
- [X] T007 [P] [US1] Write integration test `test_default_auth_disabled` in `tests/integration/api_auth_test.rs` — unauthenticated GET to `/api/energy/windows` returns 200 when `require_auth = false`
- [X] T008 [P] [US1] Write integration test `test_health_always_accessible` in `tests/integration/api_auth_test.rs` — GET `/api/health` returns 200 with no Authorization header regardless of auth setting

### Implementation for User Story 1

- [X] T009 [US1] Implement `api_key_middleware` in `src/api/middleware/api_key.rs` — when `state.api_key = None`, immediately call `next.run(request).await` with no header inspection
- [X] T010 [US1] Restructure router in `src/api/server.rs` — register `/api/health` on the outer `Router` (always exempt); move all other `/api/*` routes to an inner `protected_router` with `.route_layer(middleware::from_fn_with_state(state.clone(), api_key_middleware))`
- [X] T011 [US1] Update `main.rs` startup — when `require_auth = false`, set `AppState.api_key = None` and skip all auth setup paths

**Checkpoint**: `cargo test` passes for US1 tests. `curl http://localhost:8080/api/energy/windows` returns 200. `curl http://localhost:8080/api/health` returns 200. No `Authorization` header required.

---

## Phase 4: User Story 2 — Enable API Key Protection (Priority: P2)

**Goal**: A user adds `require_auth = true` and `api_key = "..."` (≥32 chars) to config, restarts, and all `/api/*` routes (except `/api/health`) reject requests without the correct Bearer token with a `401` whose body is identical for every failure mode.

**Independent Test**: Set `require_auth = true` with a 43-char key; `curl /api/energy/windows` (no header) → 401; `curl -H "Authorization: Bearer wrongkey"` → 401 (identical body); `curl -H "Authorization: Bearer <correct-key>"` → 200.

### Tests for User Story 2 (TDD — write first, verify they FAIL before implementation)

> **⚠️ Write these tests first. Confirm they compile but FAIL before proceeding to implementation.**

- [X] T012 [P] [US2] Write unit tests for key validation in `tests/unit/api_key_test.rs`: `test_short_key_rejected` (len < 32 → startup error), `test_empty_key_treated_as_none`, `test_whitespace_key_treated_as_none`, `test_valid_key_accepted` (len ≥ 32 → no error)
- [X] T013 [P] [US2] Write unit tests for constant-time comparison in `tests/unit/api_key_test.rs`: `test_correct_key_passes_ct_eq`, `test_wrong_key_fails_ct_eq`, `test_partial_match_fails_ct_eq` — using `subtle::ConstantTimeEq` on `&[u8]`
- [X] T014 [P] [US2] Write integration tests in `tests/integration/api_auth_test.rs`: `test_missing_header_returns_401`, `test_wrong_key_returns_401`, `test_malformed_header_returns_401`, `test_correct_key_returns_200`, and `test_all_protected_routes_return_401` — parameterized over all 7 routes in `contracts/api.md §Protected Routes` table; each route returns 401 with no Authorization header when auth is enabled (SC-005 structural coverage)
- [X] T015 [P] [US2] Write integration test `test_401_body_identical_for_all_failure_modes` in `tests/integration/api_auth_test.rs` — response body for missing header, wrong key, and malformed header are byte-identical and contain `"authentication required"`
- [X] T016 [P] [US2] Write integration test `test_configured_key_emits_auth_enabled_log` in `tests/integration/api_auth_test.rs` — start service with explicit key ≥32 chars; assert startup logs contain an `api_auth_enabled` event at INFO level with a `key_len` field equal to the key length (FR-011)

### Implementation for User Story 2

- [X] T017 [US2] Implement startup key validation in `src/main.rs` — `key.trim().is_empty()` → treat as `None`; `key.len() < 32` → `tracing::error!(event = "config_error", ...)` + `std::process::exit(2)`; `key.len() >= 32` → `tracing::info!(event = "api_auth_enabled", key_len = key.len())` + `Zeroizing<String>` + clone to `AppState`
- [X] T018 [US2] Implement 401 response and constant-time key comparison in `src/api/middleware/api_key.rs` — extract Bearer token from Authorization header; compare against stored key using `subtle::ct_eq(supplied.as_bytes(), stored.as_bytes()).into()`; all three failure modes (missing, malformed, wrong key) return identical `{"error":"authentication required","hint":"set Authorization: Bearer <api-key> header"}` body

**Checkpoint**: `cargo test` passes for all US1 and US2 tests. Short-key config causes `exit(2)`. Correct key returns 200; all wrong/missing/malformed keys return 401 with identical body.

---

## Phase 5: User Story 3 — Auto-Generated Key on First Enable (Priority: P3)

**Goal**: A user sets only `require_auth = true` (no `api_key`); the service generates a CSPRNG key at startup, logs it with the literal tag `API_KEY_GENERATED`, warns it is ephemeral, and the key works immediately for authentication.

**Independent Test**: Start service with `require_auth = true`, no `api_key`; `docker compose logs | grep API_KEY_GENERATED` returns the key on one line; use that key in `Authorization: Bearer` → 200.

### Tests for User Story 3 (TDD — write first, verify they FAIL before implementation)

> **⚠️ Write these tests first. Confirm they compile but FAIL before proceeding to implementation.**

- [X] T019 [P] [US3] Write unit tests for key generation in `tests/unit/api_key_test.rs`: `test_generate_api_key_returns_43_chars`, `test_generate_api_key_is_base64url_alphabet`, `test_two_generated_keys_are_not_equal` (basic randomness assertion)
- [X] T020 [P] [US3] Write integration tests in `tests/integration/api_auth_test.rs`: `test_autogen_key_log_contains_api_key_generated_tag`, `test_autogen_key_authenticates_successfully`, and a negative assertion `test_api_key_absent_from_subsequent_logs` — capture all log lines after the `API_KEY_GENERATED` event and assert the key value does not appear in any of them (FR-006/FR-008)
- [X] T021 [P] [US3] Write integration test `test_tls_warning_on_non_loopback` in `tests/integration/api_auth_test.rs` — start service with `require_auth = true` and a non-loopback bind address (e.g., `0.0.0.0`); assert startup logs contain an `api_tls_warning` event at WARN level (FR-012)

### Implementation for User Story 3

- [X] T022 [US3] Implement `generate_api_key()` in `src/main.rs` — `OsRng.fill_bytes(&mut [u8; 32])` then `base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)` → returns 43-char base64url `String` (≥256 bits entropy)
- [X] T023 [US3] Implement auto-gen startup path in `src/main.rs` — when `api_key = None` (or empty/whitespace): call `generate_api_key()`, emit `tracing::warn!(event = "API_KEY_GENERATED", api_key = %key)`, then emit `tracing::warn!(event = "api_key_ephemeral", message = "This key changes on every restart. Set api_key in config.toml to pin a stable key.")`; wrap key in `Zeroizing<String>`; clone plain `String` to `AppState`
- [X] T024 [US3] Implement TLS warning in `src/main.rs` — after auth is enabled, if `api.host` is not `"127.0.0.1"`, `"::1"`, or `"localhost"`, emit `tracing::warn!(event = "api_tls_warning", message = "API keys require transport encryption. This process does not terminate TLS — use a reverse proxy (e.g. Caddy).")`
- [X] T025 [US3] Apply `ZeroizeOnDrop` via the `Zeroizing<String>` wrapper from the `zeroize` crate in `src/main.rs` — the key is overwritten in memory on drop (process shutdown), satisfying FR-013

**Checkpoint**: `cargo test` passes for all US1, US2, and US3 tests. Auto-gen log line contains literal `API_KEY_GENERATED`. Generated key works for auth. Non-loopback host triggers TLS warning.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all user stories.

- [X] T026 [P] Run `cargo clippy -- -D warnings` across all modified and new files; fix all warnings in `Cargo.toml`, `src/config.rs`, `src/main.rs`, `src/api/mod.rs`, `src/api/server.rs`, `src/api/middleware/api_key.rs`
- [X] T027 [P] Run `cargo test` — confirm all tests in `tests/unit/api_key_test.rs` and `tests/integration/api_auth_test.rs` pass
- [X] T028 Validate all five `specs/002-api-key-auth/quickstart.md` scenarios (default access, auto-gen key, static key, disable auth, short-key failure) against a running service instance

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — BLOCKS all user stories
- **User Stories (Phase 3–5)**: All depend on Phase 2 completion; stories can proceed in priority order (US1 → US2 → US3)
- **Polish (Phase 6)**: Depends on all user story phases complete

### User Story Dependencies

- **US1 (P1)**: Can start after Phase 2 — no dependency on US2 or US3
- **US2 (P2)**: Can start after Phase 2 — builds on US1 middleware foundation but independently testable
- **US3 (P3)**: Can start after Phase 2 — builds on US2 startup logic; independently testable via its own log/auth assertions

### Within Each User Story

- Tests MUST be written and confirmed to FAIL before implementation
- Middleware implementation (T009, T018) before router wiring (T010)
- Startup logic (T011, T017, T022–T024) is independent of middleware implementation

---

## Parallel Execution Strategy

This plan is designed for subagent execution per `specs/002-api-key-auth/plan.md §Parallel Execution Strategy`.

### Batch 1 — Run All Three in Parallel

| Subagent | Scope | Tasks | Files |
|----------|-------|-------|-------|
| **Test-A** | All unit tests (US1–US3) | T006, T012, T013, T019 | `tests/unit/api_key_test.rs` |
| **Test-B** | All integration tests (US1–US3) | T007, T008, T014, T015, T016, T020, T021 | `tests/integration/api_auth_test.rs` |
| **Impl-A** | Config + startup logic | T011, T017, T022, T023, T024, T025 | `src/config.rs`, `src/main.rs` |

> Tests compile against the struct shapes from Phase 2 (T004, T005). Impl-A modifies `config.rs` and `main.rs` only — no overlap with test files. T011 belongs in Impl-A (not Impl-B) because it writes the `require_auth = false` branch of the same startup block in `main.rs` that T017/T022/T023 write.

### Batch 2 — Sequential After Batch 1

| Subagent | Scope | Tasks | Dependency |
|----------|-------|-------|------------|
| **Impl-B** | Middleware + router | T009, T010, T018 | Impl-A (AppState shape from T005, startup paths from T011/T017) |

> Impl-B must follow Impl-A because the middleware reads `AppState.api_key` set by startup logic in `main.rs`.

### Execution Order Diagram

```
Phase 1 (Setup: T001–T003)
    │
Phase 2 (Foundational: T004–T005)
    │
    ├── [Parallel Batch 1]
    │       Test-A:  T006, T012, T013, T019
    │       Test-B:  T007, T008, T014, T015, T016, T020, T021
    │       Impl-A:  T011, T017, T022, T023, T024, T025
    │
    └── [Sequential Batch 2]
            Impl-B:  T009, T010, T018
    │
Phase 6 (Polish: T026–T028)
```

---

## Parallel Example: Batch 1

```bash
# Launch all three subagents simultaneously after Phase 2 completes:

Task (Test-A): "Write all unit tests for api key auth in tests/unit/api_key_test.rs.
  Tests: middleware passthrough when disabled (T006), key validation — short/empty/whitespace/valid (T012),
  constant-time comparison — correct/wrong/partial (T013), key generation — length/charset/randomness (T019).
  Tests must compile and FAIL before implementation."

Task (Test-B): "Write all integration tests for api key auth in tests/integration/api_auth_test.rs.
  Tests: default auth disabled 200 (T007), health always accessible (T008),
  missing/wrong/malformed/correct header + all 7 protected routes return 401 per contracts/api.md (T014),
  identical 401 bodies (T015), configured key emits api_auth_enabled INFO log with key_len field (T016),
  autogen key in logs + authenticates + negative assertion key absent from subsequent logs (T020),
  TLS warning on non-loopback host (T021).
  Tests must compile and FAIL before implementation."

Task (Impl-A): "Implement startup logic and key generation in src/config.rs and src/main.rs.
  Paths: require_auth=false → AppState.api_key = None, skip auth (T011);
  key.trim().is_empty() → None; key.len()<32 → ERROR config_error + exit(2);
  key.len()>=32 → INFO api_auth_enabled + key_len + Zeroizing<String> + clone to AppState (T017);
  no key → generate_api_key() via OsRng + base64url + WARN API_KEY_GENERATED + WARN api_key_ephemeral (T022, T023);
  WARN api_tls_warning if non-loopback (T024); ZeroizeOnDrop via Zeroizing<String> (T025).
  Also implement generate_api_key() in src/main.rs."
```

---

## Implementation Strategy

### MVP (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: User Story 1 (tests + implementation)
4. **STOP and VALIDATE**: existing deployments unchanged
5. Ship — auth is still off; this is a zero-risk increment

### Incremental Delivery

1. Setup + Foundational → struct changes compile
2. US1 → existing behaviour preserved (MVP)
3. US2 → single-toggle protection enabled
4. US3 → auto-generated key path complete
5. Each story is independently testable; no story breaks the previous

---

## Notes

- [P] tasks = different files, no incomplete-task dependencies — safe to run simultaneously
- [US#] label maps each task to its user story for traceability back to `spec.md`
- T002 creates both `mod.rs` and an empty `api_key.rs` stub so `cargo check` passes between T002 and T009
- `Zeroizing<String>` lives only in `main.rs`; `AppState` holds a plain `String` clone for `Clone` compatibility
- `subtle::ct_eq(a.as_bytes(), b.as_bytes()).into()` is the correct call pattern — compare `&[u8]`, not `&str`
- `/api/health` must remain on the outer router to be exempt; inner router uses `.route_layer()` not `.layer()`
- Exit code 2 is the config error code; exit code 1 is the token error code (pre-existing)
- The `API_KEY_GENERATED` literal tag must appear in the `event` field so `grep API_KEY_GENERATED` is machine-findable per SC-002
- T011 is in Impl-A scope (not Impl-B) because it writes the same `main.rs` startup block as T017/T022/T023 — splitting across parallel subagents would cause a file conflict
- SC-005 coverage is via T014's `test_all_protected_routes_return_401` (structural assertion over all 7 routes in contracts/api.md), not per-route duplicate 401 tests
