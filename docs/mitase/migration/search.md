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
| `REQ-SRCH-001` | Requirement with `keyword-search` Criterion governed by `POL-012#rule.governance` | Promoted to a Verification Claim only through the exact Rust test `test_search_req_srch_001_keyword_search`, covering the exact `search_entries` implementation target. |
| `REQ-SRCH-002` | Requirement with `content-scan` Criterion governed by `POL-012#rule.governance` | Narrowed to the current Entry scan proven by exact Rust test `test_search_req_srch_002_fallback_scan`. The authorized AssetText projection, its fallback behavior, and the legacy SQL-rule test path remain migration gaps because this test does not exercise them. |
| `REQ-SRCH-003` | Requirement with `sql-rules` Criterion | Remains untraced. The legacy record has no test reference in this source slice. |
| `REQ-SRCH-004` | Requirement with `keyword-first-route` Criterion | Remains untraced. The frontend and E2E references name test files and framework cases rather than an exact selector supported by the current Mitase inventory. |
| `REQ-SRCH-005` | Requirement with `structured-search-reuse` Criterion | Remains untraced for the same selector-boundary reason as `REQ-SRCH-004`. |

The legacy `verification: traced` field is not copied as a Mitase proof state.
Only a uniquely resolved current target, a Criterion-matching implementation
target, and complete declarative runner metadata produce a Verification Claim.
Mitase does not execute the runner in this migration.
