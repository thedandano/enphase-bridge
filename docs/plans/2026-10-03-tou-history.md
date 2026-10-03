# TOU history and intervals

North Star: trustworthy dashboard timing agrees with cost estimates.
Approved: configured required IANA timezone; paginated utility/plan revisions; verified historical metadata and atomic refresh; shared resolver; half-open GET /api/tou/intervals capped at 31 days; explicit coverage errors; historical estimates with all contributing IDs. No dependencies, no deployment, no dashboard changes until real responses are verified.

## Ledger
- Task 1: configuration and shared resolver — complete. 22 calculator/configuration tests pass.
- Task 2: history and atomic refresh — complete. Pagination, rollback, migration and real-response checks pass.
- Task 3: interval endpoint and historical estimates — complete. Router and DST/cost parity checks pass.
- Task 4: full checks and independent review — complete. Reviewer found missing utility identity validation; reproduced with a failing test and fixed. Full suite and clippy rerun below.

Ruling: Work in a clean bridge checkout on a new feature branch; dashboard server remains running in its separate repository.

Ruling: Real OpenEI history contains a revision ending before it starts. Archive that row with a warning, but exclude it from resolution. This preserves upstream evidence without blocking valid history or claiming false coverage.

Ruling: Read-only GET behavior applies to the new interval endpoint. Preserve the existing estimate GET cache writes, while making their multi-revision provenance atomic.

Live verification: 10 real OpenEI revisions for the exact saved Coastal plan produced HTTP 200 for October 3, 2026, including future transitions. Local config still names TOU-DR-2, which returns no match; no configuration or deployment changed.

Review fix: De-duplicate upstream identities before excluding invalid bounds; an invalid newest copy must not revive an older copy. Regression reproduced HTTP 200 instead of 422, then passed after the fix.

Independent reviewer: gpt-5.6-sol; one Important finding fixed (required matching eiaid), no other actionable findings. No changes to the existing ignored future-formula test.

Final verification: cargo fmt --check and cargo clippy --all-targets -- -D warnings passed. cargo test: 167 passed, 1 pre-existing ignored future-formula placeholder. Final reviewed code rechecked against the 10 real OpenEI revisions and produced HTTP 200 with the correct configured timezone and current/future intervals.
