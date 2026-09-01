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

The legacy Asset registry at `features/assets.yaml` remains as read-only
migration evidence; the canonical Asset feature graph is
`docs/mitase/features/assets.yaml`.

The registry is generated from the current Rust/OpenAPI surface, not from roadmap intent. `implemented` means the route and referenced adapter exist. `contracted_unavailable` is used for `/auth/login`, whose route exists but intentionally returns `403` in this release.

Validate feature routes against [`../api/openapi.yaml`](https://github.com/ugoite/ugoite/blob/main/docs/spec/api/openapi.yaml) and referenced files against the repository. MCP is documented separately because it is not part of the portable application-operation manifest.
