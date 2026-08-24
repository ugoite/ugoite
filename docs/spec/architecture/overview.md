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
configuration and owns the small Catalog Head object boundary. `ugoite-iceberg`
constructs the official `iceberg-storage-opendal` factory, owns physical
schemas, and implements the OpenDAL-backed `SpaceCatalog`. Core receives no
OpenDAL, Iceberg, Arrow, Parquet, DataFusion, or SQL-parser types.

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

DerivedRelations use independent Relation Heads under `_ugoite/derived`, with
official Iceberg metadata and Parquet files in replaceable materialization
prefixes. They do not mutate the main Catalog Head, create Form revisions, or
enter SpaceCheckpoint. Their providers are internal unless a separate
authorization and reproducibility contract is specified.

Checkpoint capture is a read-only, reproducible coordinate over one exact
Head: it pins immutable Iceberg metadata and snapshots without claiming a
cross-Form transaction. Authorization-aware DataFusion reads and read-only
health evidence are implemented at the current service boundary. One Form
table commit is the mutation atomicity boundary; Ugoite does not claim
cross-Form transactions.

The current browser is server-backed. The target architecture adds a browser-local runtime and optional synchronization without making the server the mandatory owner of data.
