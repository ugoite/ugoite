---
title: 'Architecture overview'
---

Ugoite is a layered Rust application around portable Space directories.

```text
frontend / CLI / REST / MCP
          ↓ adapters
ugoite-api-client (remote protocol where applicable)
          ↓
ugoite-core (use cases and authorization-aware service)
          ↓
ugoite-storage + ugoite-iceberg / OpenDAL + Catalog-backed Iceberg tables
          ↓
operator-owned workspace
```

`ugoite-domain` supplies portable types and validation. `ugoite-wasm` exposes the domain/API protocol to browser JavaScript without owning fetch or persistence.
Its portable actions validate Forms and Revisions, preview Form change sets,
and derive optimistic-concurrency Revision drafts without storage access.

The target persistence model is one Iceberg namespace per Space and one
append-only table per stable Form ID. `ugoite-iceberg` owns Catalog
integration, Schema/Table Properties mapping, batch append, DataFusion
registration, migration reports, and maintenance planning. The existing Core
adapter is still being migrated from its legacy revision-row layout; it must
not be described as completing the target model until it calls this boundary
for Form and Entry operations.

Logical Form field IDs are preserved in Form metadata and an explicit mapping
property because the pinned Iceberg Rust release reassigns IDs during table
creation. Schema-bearing Form changes are rejected rather than emulated by a
rewrite: the pinned release has no public atomic schema-update transaction,
and upstream has not yet implemented `rename_column` while preserving a field
ID. This external dependency is tracked in Apache Iceberg Rust issue #2562.
Metadata-only changes remain supported.

The current browser is server-backed. The target architecture adds a browser-local runtime and optional synchronization without making the server the mandatory owner of data.
