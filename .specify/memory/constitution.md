<!--
SYNC IMPACT REPORT
==================
Version change: (template) → 1.0.0
Status: Initial ratification — no prior version

Added principles:
  I.   Rust Implementation
  II.  Test-First Engineering
  III. No Silent Failures
  IV.  Observable Operations
  V.   Incremental Delivery

Added sections:
  - Engineering Standards
  - Governance

Templates checked:
  ✅ .specify/templates/plan-template.md  — Constitution Check gates align with principles
  ✅ .specify/templates/spec-template.md  — FR-009 (Rust) and no-silent-failure FR already in spec
  ✅ .specify/templates/tasks-template.md — Phase structure and TDD task order align with Principles II & V

Deferred TODOs: None
-->

# Enphase Data Service Constitution

## North Star

> Provide a customer-centric service that is easy to use and gives the customer
> as much or as little data as they want.

Every feature, API design decision, and default configuration choice MUST be
evaluated against this statement. Complexity is only justified if it unlocks
something the customer asked for. Sensible defaults ship first; depth is
opt-in.

## Core Principles

### I. Rust Implementation

The backend service — including the data collection daemon, token management, storage layer,
and HTTP API — MUST be implemented in Rust. No alternative languages may be used for these
core components. Supporting tooling (scripts, CI automation) may use shell or other languages
where appropriate.

**Rationale**: Rust's memory safety guarantees, predictable performance, and low resource
footprint make it well suited for a long-running home server daemon. Consistency in one
language reduces cognitive overhead and dependency surface.

### II. Test-First Engineering (NON-NEGOTIABLE)

Development MUST follow the Red-Green-Refactor cycle:

- Tests are written first and MUST fail before any implementation begins.
- No feature is considered complete until tests pass.
- Unit tests cover individual functions and modules.
- Integration tests validate end-to-end polling, storage, and API flows.
- All tests run via `cargo test`; CI MUST enforce a passing test suite before merge.

**Rationale**: Tests written after implementation consistently miss edge cases and create
false confidence. This principle is the primary defence against regressions in a system
that runs unattended for days or weeks.

### III. No Silent Failures

All errors MUST be surfaced explicitly. Specifically:

- Every error MUST be logged with structured context (operation, inputs where safe, error).
- The service MUST NOT continue in a degraded state without an observable log entry.
- Retry logic MUST log each retry attempt with reason and backoff interval.
- Token refresh failures MUST halt polling and emit a clear, actionable error log.
- No fallback behavior is permitted unless it is explicitly defined, documented, and
  observable in logs.

**Rationale**: The service runs unattended. Silent failures mean data gaps that the
homeowner cannot diagnose. Fail loudly and visibly.

### IV. Observable Operations

Structured logging is required for all critical operations:

- Every polling cycle: timestamp, gateway endpoint, HTTP status, bytes received, duration.
- Every authentication event: token obtained, token refreshed, expiry time, failure reason.
- Every API request served: method, path, query params (sanitized), response status, duration.
- Every storage write: record count, timestamp range, success or failure.

Log format MUST be machine-parseable (JSON or logfmt). Logs MUST include enough context to
reconstruct system behavior without attaching a debugger.

**Rationale**: A home energy system should behave like production infrastructure. Structured
logs enable future dashboards, alerting, and audits without code changes.

### V. Incremental Delivery

Features MUST be delivered as small, independently testable vertical slices in dependency
order:

1. Authentication with the gateway (no data stored yet).
2. Data polling and local storage (headless, no API yet).
3. HTTP data API (no UI required, validated by curl or a client).
4. Optional web dashboard (only after API is stable).

Each increment MUST be demonstrable and deployable independently. No phase may begin until
the prior phase passes its independent test.

**Rationale**: Prevents the classic trap of building all layers simultaneously and
discovering integration failures late. Each increment delivers real value and reduces risk.

## Engineering Standards

The following standards apply across all phases of development:

- **Clean Code**: Functions MUST be small and single-purpose. Names MUST reveal intent.
  Duplication and hidden complexity are prohibited.
- **Modular Design**: Components MUST be loosely coupled. The polling engine, token manager,
  storage layer, and HTTP API MUST each be replaceable without cascading changes.
- **Pipeline Design**: Start with a pass-through implementation. Add functionality
  incrementally. Each pipeline stage MUST have clear input/output contracts and be
  independently testable.
- **CI/CD**: Every pull request MUST pass: linting (`clippy`), formatting (`rustfmt`),
  unit tests, integration tests, and security scanning. No merge with failing checks.
- **Pre-Commit Gates**: Linting, formatting, and static checks MUST run before every commit.
  Unit tests MUST run before every push.
- **Security**: The gateway token MUST NOT be logged in plaintext. Configuration containing
  credentials MUST be read from environment variables or a secrets file excluded from version
  control.

## Governance

- This constitution supersedes all other practices within this repository. Conflicts resolve
  in favour of the constitution.
- Amendments require: a written rationale, a version bump (semantic), and an update to
  `LAST_AMENDED_DATE`.
- MAJOR bumps (breaking principle changes) require explicit decision documentation in the
  amendment commit message.
- All implementation plans (`/speckit.plan` output) MUST include a Constitution Check gate
  before Phase 0 research begins and again after Phase 1 design.
- Compliance is reviewed at each `/speckit.clarify`, `/speckit.plan`, and `/speckit.tasks`
  invocation. Non-compliance blocks progression.
- The runtime development guidance file is `CLAUDE.md` at the repository root.

**Version**: 1.1.0 | **Ratified**: 2026-04-26 | **Last Amended**: 2026-04-26
