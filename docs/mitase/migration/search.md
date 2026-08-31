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
| `docs/spec/philosophy/foundation.yaml` plus the repository's current authority boundary | `docs/mitase/philosophies/foundation.yaml` | The four legacy philosophies remain represented beneath an explicit Foundation root for user-owned Knowledge, stable authority, disposable work, and tool/experience boundaries. The migration guard requires every canonical Policy to remain governed by the Foundation's stable-authority principle. `product_design_principle` and `coding_guideline` become named Principles so their statements and scopes remain distinct. |
| Policy `summary` and `description` | Policy `summary`, `description`, and one `governance` Rule | Policy meaning is preserved as the Rule statement. Normative strength is an explicit Ugoite decision in `docs/mitase-migration/policy-levels.yaml`, not an inferred default. Foundation, ownership, authority, compatibility, security, adapter, AI-boundary, traceability, and integration-gate rules are represented as `must` where the repository has made that decision, with exact Artifact Bindings for their declared enforcement/evidence. |
| Policy `linked_philosophies` | Rule `governed_by` | Governance is authored once on the canonical Policy Rule. Reverse views are derived by `SpecIndex`; the migration guard also requires the Foundation precedence link. |
| Policy `linked_requirements` and `linked_specifications` | Typed deferred relations in `docs/mitase-migration/policy-edges.yaml` | These legacy registry references point to domains outside this slice. Their target IDs and relation kind remain machine-readable and are not silently dropped or replaced by partial local Mitase graph nodes. |

No Ugoite-specific Mitase kind or schema extension is introduced.
The migration authority and staged-adoption rules are documented in
`docs/mitase/migration/policy.md` and enforced as repository guidance in
`AGENTS.md`.
This is a staged adoption gate: `mitase.yaml` keeps readiness and owned-change
coverage relaxed while the remaining domains migrate. The ratchet is to
complete each domain's meaning, close or expose its evidence gaps, and then
raise that domain's coverage before tightening the global gate.

## Search normalization

| Legacy record | Canonical representation | Proof decision |
| --- | --- | --- |
| `REQ-SRCH-001` | Requirement with `keyword-search` Criterion governed by `POL-012#rule.governance`, `POL-011#rule.governance`, and `POL-016#rule.governance` | Promoted to Verification Claims through the exact protected-server test `test_search_req_srch_001_authorized_route` and the exact frontend test `test_search_req_srch_004_keyword_first_route`. The canonical HTTP contract is the OpenAPI `GET /spaces/{space_id}/search` operation; the server handler, TypeScript client, and Search route are linked as participants. |
| `REQ-SRCH-002` | Requirement with Criteria for current Entry scanning, authorized AssetText projection, and missing/stale/corrupt derived-state fallback, governed by the relevant `POL-012`, `POL-011`, `POL-014`, and `POL-016` rules | The complete requirement meaning is preserved. Only the `current-entry-scan` Criterion is currently promoted through exact Rust evidence; the derived projection and fallback Criteria remain unverified until criterion-specific tests are available. |
| `REQ-SRCH-003` | Requirement with `sql-rules` Criterion governed by `POL-012#rule.governance`, `POL-014#rule.governance`, and `POL-016#rule.governance` | Remains untraced. The legacy record has no test reference in this source slice. |
| `REQ-SRCH-004` | Requirement with `keyword-first-route` Criterion governed by `POL-012#rule.governance`, `POL-014#rule.governance`, and `POL-016#rule.governance` | Promoted to a Verification Claim through the exact frontend test `test_search_req_srch_004_keyword_first_route`, covering the current `SpaceSearchRoute` target. The E2E reference remains a migration gap. |
| `REQ-SRCH-005` | Requirement with `structured-search-reuse` Criterion governed by `POL-012#rule.governance`, `POL-014#rule.governance`, and `POL-016#rule.governance` | Promoted to Verification Claims through the exact frontend tests `test_search_req_srch_005_advanced_search_compiles` and `test_search_req_srch_005_history_reuse`, covering the current `SpaceSearchRoute` and `buildAdvancedSearchQuery` targets. The E2E reference remains a migration gap. |

The legacy `verification: traced` field is not copied as a Mitase proof state.
Only a uniquely resolved current target, a Criterion-matching implementation
target, and complete declarative runner metadata produce a Verification Claim.
Migration never narrows a Requirement or Criterion to match available proof;
missing proof remains an unverified Criterion and an explicit migration gap.
The frontend test callbacks are named top-level symbols so the TypeScript
inventory can resolve them exactly; the declarative `frontend-vitest` runner
selects the migrated Search test file and test title without executing it in
Mitase. Mitase does not execute any runner in this migration.
