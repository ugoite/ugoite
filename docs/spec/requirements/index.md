---
title: 'Requirements registry'
---

Requirement YAML files define stable IDs, descriptions, governance links, implementation status, and generated test traceability.

A current test mapping has this shape:

```yaml
verification: traced
tests:
  - file: crates/ugoite-core/tests/test_entry.rs
```

When no current source/test file contains the requirement ID:

```yaml
verification: untraced
```

Do not preserve references to deleted tests. Planned requirements may remain untraced. Any requirement that describes a removed architecture should be rewritten to the current behavior or marked superseded.
