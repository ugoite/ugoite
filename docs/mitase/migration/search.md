---
title: 'Search migration ledger'
description: 'Evidence and explicit gaps for the first live Mitase migration slice.'
---

This page records the evidence and explicit gaps for the first live Search
migration from Ugoite's legacy specification registry into Mitase.

## Search migration ledger

This ledger records the first live migration slice from Ugoite's legacy
registry into the canonical Mitase graph. The source is pinned to
`a872f4992bcb3633681eb0383e101453f00b32db` and remains available under
`docs/spec/` until the remaining domains are migrated.

## Governance normalization

| Legacy source | Canonical Mitase representation | Decision |
| --- | --- | --- |
| `docs/spec/philosophy/foundation.yaml` | `docs/mitase/philosophies/foundation.yaml` | All four philosophies are represented. `product_design_principle` and `coding_guideline` become named Principles so their statements and scopes remain distinct. |
| Policy `summary` and `description` | Policy `summary`, `description`, and one `governance` Rule | The policy meaning is preserved as the Rule statement. The source does not declare a Mitase RuleLevel, so this slice uses the conservative `should` level. |
| Policy `linked_philosophies` | Rule `governed_by` | Governance is authored once on the canonical Policy Rule. Reverse views are derived by `SpecIndex`. |
| Policy `linked_requirements` and `linked_specifications` | Migration gap | These legacy registry references point to domains outside this slice. They remain in the pinned source files and are not silently invented as partial Mitase graph nodes. |

No Ugoite-specific Mitase kind or schema extension is introduced.

## Search normalization

| Legacy record | Canonical representation | Proof decision |
| --- | --- | --- |
| `REQ-SRCH-001` | Requirement with `keyword-search` Criterion governed by `POL-012#rule.governance` | Promoted to Verification Claims through the exact protected-server test `test_search_req_srch_001_authorized_route` and the exact frontend test `test_search_req_srch_004_keyword_first_route`. The canonical HTTP contract is the OpenAPI `GET /spaces/{space_id}/search` operation; the server handler, TypeScript client, and Search route are linked as participants. |
| `REQ-SRCH-002` | Requirement with `content-scan` Criterion governed by `POL-012#rule.governance` | Narrowed to the current Entry scan proven by exact Rust test `test_search_req_srch_002_fallback_scan`. The authorized AssetText projection, its fallback behavior, and the legacy SQL-rule test path remain migration gaps because this test does not exercise them. |
| `REQ-SRCH-003` | Requirement with `sql-rules` Criterion | Remains untraced. The legacy record has no test reference in this source slice. |
| `REQ-SRCH-004` | Requirement with `keyword-first-route` Criterion | Promoted to a Verification Claim through the exact frontend test `test_search_req_srch_004_keyword_first_route`, covering the current `SpaceSearchRoute` target. The E2E reference remains a migration gap. |
| `REQ-SRCH-005` | Requirement with `structured-search-reuse` Criterion | Promoted to Verification Claims through the exact frontend tests `test_search_req_srch_005_advanced_search_compiles` and `test_search_req_srch_005_history_reuse`, covering the current `SpaceSearchRoute` and `buildAdvancedSearchQuery` targets. The E2E reference remains a migration gap. |

The legacy `verification: traced` field is not copied as a Mitase proof state.
Only a uniquely resolved current target, a Criterion-matching implementation
target, and complete declarative runner metadata produce a Verification Claim.
The frontend test callbacks are named top-level symbols so the TypeScript
inventory can resolve them exactly; the declarative `frontend-vitest` runner
selects the migrated Search test file and test title without executing it in
Mitase. Mitase does not execute any runner in this migration.
