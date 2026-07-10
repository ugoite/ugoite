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

Each Space is an Iceberg namespace and each stable Form ID owns exactly one
append-only table. `ugoite-iceberg` owns Catalog integration, Schema/Table
Properties mapping, batch append, DataFusion registration, migration reports,
and maintenance planning. `ugoite-core` owns authorization and use-case
orchestration; it does not discover current metadata by listing object paths.

The current browser is server-backed. The target architecture adds a browser-local runtime and optional synchronization without making the server the mandatory owner of data.
