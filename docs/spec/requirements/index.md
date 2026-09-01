---
title: 'Requirements registry'
---

Requirement YAML files define stable IDs, descriptions, governance links, implementation status, and generated test traceability. Migrated domains are authoritative in `docs/mitase`; this registry remains authoritative for domains that have not yet been migrated.

The migrated Entry, Form, and Search requirements retain their external
operator/API contracts in the canonical graph; an exact test claim is added
only for the behavior that the selected test actually exercises. Search
authorization remains governed by the authoritative Security requirement until
that domain is migrated.

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
