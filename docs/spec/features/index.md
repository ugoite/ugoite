---
title: 'Feature registry'
---

`features.yaml` indexes per-area YAML files. Each operation records the canonical HTTP method/path and existing backend, frontend, core, and optional CLI source locations.

The Entry, Form, Search, API, and Asset operation maps are represented canonically in
`docs/mitase/features`; this file and the legacy per-area registries remain
read-only migration snapshots for those domains. Other domains remain
authoritative here until their replacement review is complete. The canonical
feature graph retains core/service, backend, frontend, CLI, and HTTP contract
surfaces rather than treating a core implementation target as the whole
capability.

The API-specific legacy registries for spaces, preferences, and SQL are retained
for migration evidence but are no longer included in Mitase's declared
inventory. The shared `features.yaml` index remains because it also indexes
unmigrated domains.

The legacy Asset feature registry at `features/assets.yaml` is retained as a
read-only migration snapshot and is no longer included in Mitase's declared
inventory. The canonical Asset feature graph at `docs/mitase/features/assets.yaml`
is the only semantic authority for the migrated Asset domain.

The E2E browser feature is represented by the canonical graph at
`docs/mitase/features/e2e.yaml`; no legacy per-area E2E feature registry is
authoritative for that domain.

The Space storage foundation and connector/access/routing/preference feature
slice is represented by the canonical graph at
`docs/mitase/features/storage.yaml` plus the shared API operation bindings in
`docs/mitase/features/api.yaml`. Its corresponding legacy Space operation
records remain read-only migration evidence for the migrated Storage slice.
Duplicate-create conflict semantics remain in the legacy Space records until
their HTTP 409 contract is reconciled with the current idempotent bootstrap
retry behavior; storage layout synchronization and remaining derived or
compatibility operations continue to await focused reviews.
The connector feature records the existing server and settings surfaces plus
the shared connection probe; mandatory pre-commit sequencing remains an
explicit evidence gap. The accessible-listing feature similarly keeps runtime
authorization and storage-error behavior on the server binding while tracing
the published API shape separately.

The registry is generated from the current Rust/OpenAPI surface, not from roadmap intent. `implemented` means the route and referenced adapter exist. `contracted_unavailable` is used for `/auth/login`, whose route exists but intentionally returns `403` in this release.

Validate feature routes against [`../api/openapi.yaml`](https://github.com/ugoite/ugoite/blob/main/docs/spec/api/openapi.yaml) and referenced files against the repository. MCP is documented separately because it is not part of the portable application-operation manifest.
