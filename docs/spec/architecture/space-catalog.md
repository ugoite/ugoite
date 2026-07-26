---
title: 'SpaceCatalog publication contract'
---

This is the single durable-control-plane contract for a Ugoite Space. It is
intentionally destructive before the first release: the internal pre-release
Space format version is unchanged, while legacy layouts and catalog modes are
unsupported rather than migrated.

## Authority and ownership

One Space maps to one Iceberg namespace. One stable Form ID maps to one
append-only Iceberg revision table. Iceberg metadata and revision tables are
authoritative for table history and state derivation.

All durable Ugoite objects are reachable through the configured OpenDAL Space
boundary. Ugoite does not require PostgreSQL, SQLite, a SQL Catalog, Hive
Metastore, or any other hosted durable service. The server exposes a Space; it
does not own one.

The only mutable catalog root is `_ugoite/catalog/head.json`. Immutable
publication records live at `_ugoite/catalog/publications/<generation>-<command-id>.json`.
Only records reachable from Head through `previous_publication` are authoritative.
Object listing never establishes current state or order.

## Exact reads and publication

An exact Head read first obtains a real ETag with `stat`, then reads Head with
that ETag as `if_match`; a changed object restarts the read. A plain stat plus
unconditional read is not exact.

A writer reads one exact Head and referenced table metadata, validates its
domain command, prepares immutable Iceberg data and metadata through the
official upstream writer, writes one immutable publication record, and replaces
Head with `if_match` using the exact ETag. No automatic refresh/rebase changes a
logical mutation. An ETag mismatch reloads exact state and re-runs domain
validation.

A transport error after Head replacement has an unknown outcome. The writer
rereads Head and walks reachable publication records back to its base
generation: a matching command identity proves success; reaching base without
it proves non-publication; missing or corrupt evidence is an explicit
unknown/corrupt result. Repeating the mutation blindly is forbidden.

## Capability and concurrency boundary

Shared writes require behavioral probes, not merely capability flags: a real
changing ETag, exact conditional read, conditional create-if-absent,
conditional replacement, and stale-ETag rejection. Unsupported backends fail
closed for shared writes. Explicit single-process mode may use local
serialization but still writes every durable byte through OpenDAL.

Readers never lock. Writers may prepare immutable files concurrently. Visibility
changes only through Head. One Form table commit is the mutation atomicity unit;
Ugoite makes no cross-Form transaction claim. Leases, TTLs, heartbeats, lock
files, fences, custom FileIO, metadata reconstruction, object-list recovery,
and custom maintenance engines are outside this architecture.

## Crate boundary

`ugoite-storage` owns normalized storage configuration, the Catalog Head
operator, capability probes, and the small conditional-object API. It does not
become a generic OpenDAL wrapper.

`ugoite-iceberg` owns `iceberg-storage-opendal`, `SpaceCatalog`, physical
schemas, typed Arrow conversion, upstream writers, table commits, snapshots,
DataFusion, checkpoints, and health evidence. Its physical upstream types do
not cross into Core.

`ugoite-core` owns domain validation, authorization meaning, use-case
orchestration, domain commands, receipts, checkpoints, and errors. It neither
constructs nor inspects OpenDAL, Iceberg, Arrow, Parquet, DataFusion, or SQL
parser objects.
