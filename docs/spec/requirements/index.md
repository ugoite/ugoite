---
title: 'Requirements registry'
---

Requirement YAML files define stable IDs, descriptions, governance links, implementation status, and generated test traceability. Migrated domains are authoritative in `docs/mitase`; this registry remains authoritative for domains that have not yet been migrated.

The migrated Entry, Form, Search, API, Asset, and E2E requirements retain their external
operator/API contracts in the canonical graph; an exact test claim is added
only for the behavior that the selected test actually exercises. Search
authorization remains governed by the authoritative Security requirement until
that domain is migrated. API adapter and frontend verification gaps remain
explicit follow-up work rather than being inferred from the existence of a
binding.

The legacy API requirement registry at `requirements/api.yaml` is retained as a
read-only migration snapshot and is no longer included in Mitase's declared
inventory. The canonical API requirements at `docs/mitase/requirements/api.yaml`
are the only semantic authority for the migrated API domain.

The legacy Asset requirement registry at `requirements/asset.yaml` is retained
as a read-only migration snapshot and is no longer included in Mitase's declared
inventory. The canonical Asset requirements at `docs/mitase/requirements/assets.yaml`
are the only semantic authority for the migrated Asset domain.

The legacy E2E requirement registry at `requirements/e2e.yaml` is retained as a
read-only migration snapshot and is no longer included in Mitase's declared
inventory. The canonical E2E requirements at `docs/mitase/requirements/e2e.yaml`
are the only semantic authority for the migrated E2E domain.

The legacy Integrity requirement registry at `requirements/integrity.yaml` is
retained as a read-only migration snapshot. The canonical Integrity requirements
at `docs/mitase/requirements/integrity.yaml` are the only semantic authority for
the migrated Integrity domain.

The Storage Space foundation slice (`REQ-STO-001`, `REQ-STO-002`, `REQ-STO-003`,
`REQ-STO-004`, and `REQ-STO-007`) is represented canonically at
`docs/mitase/requirements/storage.yaml`. `REQ-STO-005` remains authoritative in
this legacy registry because its broad HTTP 409 duplicate contract must first be
reconciled with the current idempotent bootstrap-retry behavior. Connector,
preference, directory-resilience, derived-relation, and Knowledge-compatibility
Storage requirements remain in this registry until their later migration slices
are reviewed.

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
