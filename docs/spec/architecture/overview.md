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
ugoite-storage (OpenDAL configuration, Space keys, and publication objects)
          ↓
operator-owned workspace
```

`ugoite-domain` supplies portable types, strict Space-relative keys, and logical
Space URIs. `ugoite-wasm` exposes the domain/API protocol to browser JavaScript
without owning fetch or persistence.
Its portable actions validate Forms and Revisions, preview Form change sets,
and derive optimistic-concurrency Revision drafts without storage access.

The persistence model is one Iceberg namespace per Space and one append-only
revision table per stable Form ID. `ugoite-storage` normalizes backend
configuration and owns the small Catalog Head/publication object boundary. Its
`PublicationStore` exposes opaque revisions and backend-neutral create/CAS
outcomes without leaking ETags or backend schemes upward. `ugoite-iceberg`
owns the logical-coordinate FileIO bridge, physical schemas, and the OpenDAL-
backed `SpaceCatalog`. Iceberg locations are persisted as
`ugoite://{space_uid}/{space-relative-key}` and resolved only against the
operator bound to that Space. Core receives no OpenDAL, Iceberg, Arrow, Parquet,
DataFusion, or SQL-parser types.

The portable `PublicationStore` contract is the storage-boundary foundation.
Local stores use their single-process serializer; non-local authoritative
mutations receive a permit only after the storage boundary verifies exact
read, create-if-absent, conditional replacement, stale-revision rejection,
and one-winner concurrent CAS behavior. Authorization mutations use an exact
`AuthorizationState` load followed by its own single-object CAS; no durable
lease, heartbeat, or cross-object write fence is required for ordinary
mutations.

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

User-visible Knowledge publications also carry one immutable `ChangeDescriptor`.
The publication `command_id` is the Change ID, so history is reconstructed
from the reachable publication chain without a second Change index. `RunId` is
correlation metadata only; it has no durable status record. The active Pin map is
part of the Catalog Head and each Pin stores a `PublicationRef` to the exact
immutable publication it names. Pin reads therefore use the Head, never object
listing or a separate checkpoint registry. Selective revert is append-only: it
plans a new Change from field-level before/after/current comparisons and never
rewinds Head.

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
