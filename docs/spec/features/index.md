---
title: 'Feature registry'
---

`features.yaml` indexes per-area YAML files. Each operation records the canonical HTTP method/path and existing backend, frontend, core, and optional CLI source locations.

The Entry, Form, Search, API, Asset, and Frontend operation maps are
represented canonically in `docs/mitase/features`. Retired per-area feature
registries are not second authorities; remaining legacy requirements and other
evidence stay read-only until the broader `docs/spec` cleanup is complete.
Other domains remain authoritative here until their replacement review is
complete. The canonical feature graph retains core/service, backend,
frontend, CLI, and HTTP contract surfaces rather than treating a core
implementation target as the whole capability.

The legacy Entry feature registry is retired; Entry lifecycle semantics are
owned by `docs/mitase/features/entries.yaml` and its linked requirements, while
the legacy Entry requirement registry remains read-only migration evidence.

The legacy Form feature registry is retired; Form schema and operation
semantics are owned by `docs/mitase/features/forms.yaml` and its linked
requirements, while the legacy Form requirement registry remains read-only
migration evidence.

The legacy Search feature registry is retired; keyword and structured-query
semantics are owned by `docs/mitase/features/search.yaml` and its linked
requirements, while the legacy Search requirement registry remains read-only
migration evidence.

Operations, local-run, and quality-gate surfaces for `REQ-OPS-001` through
`REQ-OPS-007` are represented canonically by
`docs/mitase/features/ops.yaml`. The legacy
records remain read-only migration evidence for this slice; later Operations
requirements remain outside its scope.

The legacy SQL registry is retained for migration evidence but is no longer
included in Mitase's declared inventory or linked from the shared `features.yaml`
index. The Spaces registry is retired because its implemented Space CRUD,
membership, storage, and catalog Pin semantics are represented by the canonical
Mitase graphs. The preference registry is retired because its implemented API
and storage semantics are represented by the canonical Mitase graphs. The shared index
remains because it also indexes unmigrated domains.

The legacy Asset feature registry at `features/assets.yaml` is retired because
the canonical Asset feature graph at `docs/mitase/features/assets.yaml` is the
only semantic authority for the migrated Asset domain. The legacy Asset
requirement registry remains separately documented as read-only migration
evidence until the broader `docs/spec` cleanup is complete.

The canonical Frontend feature graph at `docs/mitase/features/frontend.yaml` is
the only semantic authority for the migrated Frontend domain. The legacy
Frontend requirement registry remains read-only migration evidence until the
broader `docs/spec` cleanup is complete.

The E2E browser feature is represented by the canonical graph at
`docs/mitase/features/e2e.yaml`; no legacy per-area E2E feature registry is
authoritative for that domain.

The OIDC authentication and external identity linking slice remains connected
to the canonical security graph at `docs/mitase/features/security.yaml` as
`FEAT-SEC-005`.

The complete shipped authentication and operator credential surface is now
represented by `docs/mitase/features/authentication.yaml`. Its planned
account/agent boundary remains explicitly planned; the legacy
`features/auth.yaml` registry is retired rather than retained as a second
authority.

The Space storage foundation, authenticated creation contract, and connector/access/routing/preference feature
slice is represented by the canonical graph at
`docs/mitase/features/storage.yaml` plus the shared API operation bindings in
`docs/mitase/features/api.yaml`. The former legacy Space operation registry is
retired for the migrated Storage slice; remaining legacy requirement snapshots
are read-only migration evidence.
The canonical graph records HTTP 201 creation, HTTP 200 same-account retry,
and HTTP 409 duplicate-slug conflict semantics. The storage layout, DerivedRelation, and v0.1 Knowledge
compatibility operations are now represented in `docs/mitase/features/storage.yaml`
and remain here only as read-only migration evidence. Complete executable
parity between every documented layout path and runtime creation remains an
explicit follow-up rather than an inferred guarantee.
The legacy DerivedRelation feature registry is retired; its semantic authority
is the canonical storage and Asset graph in `docs/mitase/features`, while the
remaining legacy requirement snapshots continue as read-only migration
evidence.
The connector feature records the existing server and settings surfaces plus
the shared connection probe; mandatory pre-commit sequencing remains an
explicit evidence gap. The accessible-listing feature similarly keeps runtime
authorization and storage-error behavior on the server binding while tracing
the published API shape separately.

The registry is generated from the current Rust/OpenAPI surface, not from roadmap intent. `implemented` means the route and referenced adapter exist. `contracted_unavailable` is used for `/auth/login`, whose route exists but intentionally returns `403` in this release.

Validate feature routes against the server OpenAPI artifact
([`crates/ugoite-server/src/openapi.json`](https://github.com/ugoite/ugoite/blob/main/crates/ugoite-server/src/openapi.json)) and referenced files against the repository. MCP is documented separately because it is not part of the portable application-operation manifest.
