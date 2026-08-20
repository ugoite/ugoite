---
title: "Derived relations"
sidebar:
  order: 4
---

Ugoite has two sibling kinds of typed Iceberg relation:

- a Form is authoritative, append-only user data and participates in Catalog
  Head and SpaceCheckpoint coordinates;
- a DerivedRelation is a replaceable current build computed from
  authoritative Space data.

Derived data remains inside the operator-owned Space. It is not a second
database, an ACL authority, a Form, or a user-visible revision history. A
relation is valid only when deleting it and rebuilding it from authoritative
data restores the same kind of result.

## Relation Head

Built-in relations are created lazily below:

```text
spaces/{space_id}/_ugoite/derived/relations/{relation_id}/
├── head.json
└── builds/{build_id}/
    ├── metadata/                 # official Iceberg metadata
    ├── data/                     # official Parquet data files
    └── manifest.json             # Ugoite integrity inventory
```

Each relation has an independent mutable `head.json`. It contains the
definition and producer fingerprints, compatibility epoch, build identity,
Iceberg table identifier/UUID/metadata location/snapshot, source and
generation coordinates, an opaque shared-GC CAS fence, and a checksum. It does
not contain Entry revision,
author, ACL, publication-chain, or checkpoint-retention semantics.

Readers establish visibility from this Head only. Object listing is reserved
for diagnostics and garbage-collection candidate discovery. A shared writer
must prove OpenDAL ETag-bound exact read, create-if-absent, conditional
replacement, and stale rejection before using Head CAS. Single-process mode
uses a process-local serializer while keeping all durable I/O on OpenDAL.

A rebuild writes a staging build completely, validates it, then performs one
conditional Head swap. A failed or losing build is never visible. After the
swap the old build is garbage; it is retained only for a conservative grace
period so readers that already opened it can finish, then its prefix is deleted.
There is no DerivedRelation rollback, history, or generation-retention API.
An uncertain CAS outcome is resolved by exact Head reread and `build_id`; it
does not require the authoritative Catalog publication chain.

The pre-release materializations Head format is not read as a compatibility
format. A local rebuild keeps that disposable coordinate pinned while staging,
then replaces it with the new current-build Head after validation; a shared
rebuild uses its exact ETag for the same swap. The detached materialization
prefix is then retried by derived maintenance until cleanup succeeds.

## AssetText

`ugoite.asset_text` is the first internal DerivedRelation. Its producer reads
current Form/Entry AssetReferences, verifies the referenced object size and
SHA-256, and extracts bounded text from plain text, Markdown, PDF text layers,
DOCX, XLSX, and PPTX. OCR, legacy Office formats, macros, executable content,
external relationships, network fetches, and embedded execution are not part
of this producer.

The relation has a typed Iceberg schema with stable field IDs for Asset
ID/checksum/size, parser and producer identity, status, chunk index/locator,
text, Unicode text length, parse time, and coarse error code. One unique Asset
is parsed once per rebuild; Entry/Form identity is resolved from authoritative
current AssetReferences at search time. Parser output is normalized deterministically and
chunked at semantic boundaries with a bounded maximum size. Failure produces a
diagnostic row and never rolls back the authoritative Asset upload or Entry
commit.

The rebuild source is the current authoritative Entry view, not an `assets/`
object listing. Deleted or orphaned references therefore cannot seed a new
build. AssetText rows do not carry ACLs: Quick Search first obtains
authorized current Entries, joins their AssetReferences to the internal
AssetText provider, and returns the Entry as the primary result. If the derived
relation is missing, stale, corrupt, or unavailable, native Entry search still
works and AssetText search is degraded.

The internal DataFusion provider is not registered in Saved SQL, SQL Sessions,
the public relation namespace, or SpaceCheckpoint-pinned query plans. Iceberg
provides typed storage, snapshots, metadata pruning, portability, and a
DataFusion provider; it is not a persistent substring full-text index. A future
inverted index is a separate layer over the AssetText projection.

## Recovery and ownership

Deleting `_ugoite/derived` does not damage Form, Entry, Asset, or checkpoint
reads. `ugoite index run <space> --component asset-text` (or the default index
run) rebuilds the built-in relation. `ugoite index stats` reports derived
health separately from authoritative Space health. Missing derived state is a
recoverable cache condition, not Space corruption.
