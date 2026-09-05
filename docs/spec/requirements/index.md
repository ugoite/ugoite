---
title: 'Requirements registry'
---

Requirement YAML files define stable IDs, descriptions, governance links, implementation status, and generated test traceability. Migrated domains are authoritative in `docs/mitase`; this registry remains authoritative for domains that have not yet been migrated.

The migrated Entry, Form, Indexer, Search, API, Asset, Frontend, and E2E requirements retain their external
operator/API contracts in the canonical graph; an exact test claim is added
only for the behavior that the selected test actually exercises. Search
authorization remains governed by the authoritative Security requirement until
that domain is migrated. Any remaining verification gap remains explicit rather
than being inferred from the existence of a binding.

Operations requirements `REQ-OPS-001` through `REQ-OPS-007` are now canonical in
`docs/mitase/requirements/ops.yaml`. The
legacy `requirements/ops.yaml` records remain read-only migration evidence;
later Operations requirements are not included in this slice. The canonical
graph connects generic guides, workflows, settings, registries, and exact
docsite, repository-gate, and CLI evidence while preserving unverified
completeness as an explicit gap.

The legacy API requirement registry at `requirements/api.yaml` is retired and
is no longer included in Mitase's declared inventory. The canonical API
requirements at `docs/mitase/requirements/api.yaml` are the only semantic
authority for the migrated API domain.

The legacy Asset requirement registry at `requirements/asset.yaml` is retired and
is no longer included in Mitase's declared inventory. The canonical Asset
requirements at `docs/mitase/requirements/assets.yaml` are the only semantic
authority for the migrated Asset domain.

The legacy E2E requirement registry at `requirements/e2e.yaml` is retired and
is no longer included in Mitase's declared inventory. The canonical E2E
requirements at `docs/mitase/requirements/e2e.yaml` are the only semantic
authority for the migrated E2E domain.

The legacy Indexer requirement registry at `requirements/index.yaml` is retired
and is no longer included in Mitase's declared inventory. Its derived-index and
structured-query semantics are represented by the canonical Search and Form
requirements at `docs/mitase/requirements/search.yaml` and
`docs/mitase/requirements/forms.yaml`.

The legacy Search requirement registry at `requirements/search.yaml` is retired
and is no longer included in Mitase's declared inventory. The canonical Search
requirements at `docs/mitase/requirements/search.yaml` are the only semantic
authority for keyword search, structured query, frontend search behavior, and
derived relation maintenance.

The legacy Frontend requirement registry at `requirements/frontend.yaml` is
retained as a read-only migration snapshot and is no longer included in Mitase's
declared inventory. The canonical Frontend requirements at
`docs/mitase/requirements/frontend.yaml` are the only semantic authority for
the migrated Frontend domain.

The OIDC external identity requirement `REQ-SEC-016` is represented canonically
at `docs/mitase/requirements/security.yaml`. Its corresponding record in the
legacy Security registry remains read-only migration evidence for this slice;
the remaining Security requirements continue to use their existing authority.
The owner-approved Space Access Recovery regression now verifies that the old
account's OIDC methods remain unchanged while the recovered Space binding moves
to the fresh account.

The legacy Integrity requirement registry at `requirements/integrity.yaml` is
retained as a read-only migration snapshot. The canonical Integrity requirements
at `docs/mitase/requirements/integrity.yaml` are the only semantic authority for
the migrated Integrity domain.

The Storage Space foundation, creation contract, and connector/access/routing/preference slice
(`REQ-STO-001`, `REQ-STO-002`, `REQ-STO-003`, `REQ-STO-004`, `REQ-STO-005`, `REQ-STO-006`,
`REQ-STO-007`, `REQ-STO-008`, `REQ-STO-009`, `REQ-STO-010`, and `REQ-STO-011`)
is represented canonically at `docs/mitase/requirements/storage.yaml`.
`REQ-STO-005` is now canonical: the same account-bound retry is HTTP 200,
new creation is HTTP 201, and a different account's duplicate slug claim is
HTTP 409 with `SPACE_ALREADY_EXISTS`. The legacy record remains a read-only
migration snapshot. Storage layout synchronization,
derived-relation, and Knowledge-compatibility requirements remain in this
registry until their later migration slices are reviewed.
The canonical Storage connector record preserves the connector-update and
pre-commit validation contract; its available API/UI surface and core probe
evidence are traced, while mandatory sequencing of an update after successful
validation remains an explicit evidence gap. The canonical accessible-listing
record likewise keeps runtime authorization and storage-error propagation as
implementation requirements while its current verification target covers only
the published OpenAPI boundary.

The Storage layout, DerivedRelation, and v0.1 Knowledge compatibility records
(`REQ-STO-012`, `REQ-STO-013`, and `REQ-STO-014`) are now represented in
`docs/mitase/requirements/storage.yaml`. The records in this legacy file are
retained as read-only migration evidence; the canonical graph carries the
current artifact bindings and exact verification claims. Complete executable
parity between every documented layout path and runtime creation remains an
explicit follow-up rather than an inferred guarantee.

A current test mapping has this shape:

```yaml
verification: traced
tests:
  - file: crates/ugoite-iceberg/tests/test_entry.rs
```

When no current source/test file contains the requirement ID:

```yaml
verification: untraced
```

Do not preserve references to deleted tests. Planned requirements may remain untraced. Any requirement that describes a removed architecture should be rewritten to the current behavior or marked superseded.
