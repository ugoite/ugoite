---
title: 'Architecture overview'
---

Ugoite is a layered Rust application around portable Space directories.

```text
frontend / CLI / REST / MCP
          ↓ adapters
ugoite-api-client (remote protocol where applicable)
          ↓
ugoite-core (domain use cases and authorization policy)
          ↓
ugoite-iceberg (physical adapter and publication coordinator)
          ↓
ugoite-storage (OpenDAL configuration and conditional Head objects)
          ↓
operator-owned workspace
```

`ugoite-domain` supplies portable types and validation. `ugoite-wasm` exposes the domain/API protocol to browser JavaScript without owning fetch or persistence.
Its portable actions validate Forms and Revisions, preview Form change sets,
and derive optimistic-concurrency Revision drafts without storage access.

The persistence model is one Iceberg namespace per Space and one append-only
revision table per stable Form ID. `ugoite-storage` normalizes backend
configuration, constructs the Catalog Head operator, probes conditional-object
behavior, and exposes only the small exact-read/create/replace boundary.
`ugoite-iceberg` constructs the official `iceberg-storage-opendal` factory,
owns physical schemas and typed Arrow conversion, implements the one
OpenDAL-backed `SpaceCatalog`, and is the only production gateway for Iceberg
table publication and DataFusion providers. `ugoite-core` deals only in domain
commands, authorization/query policy, checkpoints, receipts, and errors.

`_ugoite/catalog/head.json` is the sole mutable catalog authority. Every Head
generation names the Form tables and their immutable Iceberg metadata. A
checksum-protected immutable publication record records the complete next Head,
the previous publication coordinate, and the command identity. Writers read
Head with its actual ETag, prepare immutable Iceberg objects, and make them
visible by a conditional Head replacement. A conflict re-runs domain validation
against an exact fresh Head; a lost response is resolved from the reachable
publication chain instead of blindly repeating the logical mutation. REST,
Memory, pointer-manifest, external-catalog, and object-list reconstruction modes
are not production architecture.

Logical Form field IDs are Iceberg field IDs. Current state is derived once from
the revision table and fails explicitly on duplicate maximum versions. Reads use
snapshot-pinned upstream Iceberg/DataFusion providers; query sessions and
checkpoints are derived/pinned state, not catalog authority. One Form table
commit is the atomicity boundary: Ugoite does not claim cross-Form transactions.

The current browser is server-backed. The target architecture adds a browser-local runtime and optional synchronization without making the server the mandatory owner of data.
