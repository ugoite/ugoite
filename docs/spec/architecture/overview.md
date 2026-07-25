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

The persistence model is one Iceberg namespace per Space and one append-only
table per stable Form ID. `ugoite-iceberg` owns Catalog integration,
Schema/Table Properties mapping, Revision-only batch append, DataFusion
registration, migration reports, and maintenance planning. Form, Entry,
Saved SQL, and SQL-session persistence in Core call this boundary; the old
revision-row writer remains only as migration-reader code and is not a
production write path.

Logical Form field IDs are sent as Iceberg field IDs and retained in Form
metadata. Schema-bearing changes use an explicitly injected REST schema
committer built from the same typed configuration as the Catalog. It resolves
the Catalog config response (including endpoint prefix), carries configured
headers and authentication, and sends `UuidMatch`, current-schema, and
last-assigned-field requirements together with the schema and Form properties
in one atomic commit. The local MemoryCatalog fallback is kept for offline
single-process CLI/test operation. It writes a next Iceberg metadata version
and then swaps the local catalog pointer, preserving existing data files and
snapshots while making newly added nullable fields available.

The current browser is server-backed. The target architecture adds a browser-local runtime and optional synchronization without making the server the mandatory owner of data.
