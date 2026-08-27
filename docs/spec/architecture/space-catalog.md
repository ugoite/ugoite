---
title: 'SpaceCatalog publication contract'
---

This is the single durable-control-plane contract for a Ugoite Space. It is
intentionally destructive before the first release: the internal pre-release
Space format version is unchanged, while legacy layouts and catalog modes are
unsupported rather than migrated.

The Catalog Head layout, upstream physical boundary, single mutation
coordinator, checkpoint-pinned revision reads, authorization-aware DataFusion
context, and read-only health evidence are implemented by the current
persistence work.

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

DerivedRelation Heads under `_ugoite/derived/relations/{relation_id}/head.json`
are independent, non-authoritative pointers. They may be absent, stale, or
replaced without changing this Catalog Head or any SpaceCheckpoint. Relation
derived builds use the official Iceberg Rust FileIO and are visible only
after their own Head CAS.

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

The record stores base/new metadata locations plus the base and resulting
Iceberg snapshot and schema IDs from upstream table metadata. Snapshot IDs may
be null for metadata-only changes or a newly created table; schema IDs make the
schema coordinate explicit without reproducing Iceberg's history locally.

A transport error after Head replacement has an unknown outcome. The writer
rereads Head and walks reachable publication records back to its base
generation: a matching command identity proves success; reaching base without
it proves non-publication; missing or corrupt evidence is an explicit
unknown/corrupt result. Repeating the mutation blindly is forbidden.

`SpaceCommitCoordinator` is the only production mutation entry point. It takes
a stable command ID, kind, and canonical domain-command digest, constructs one
short-lived publication-authorized `SpaceCatalog` per attempt, and returns the
actual command ID, Catalog generation, and Iceberg snapshot ID. Reusing a
command ID with a different digest fails. Direct physical Catalog and Arrow
mutation APIs are private to `ugoite-iceberg`; every ETag conflict starts a
fresh attempt and reruns the domain validation before publishing.

## Capability and concurrency boundary

Shared writes require behavioral probes, not merely capability flags: a real
changing ETag, exact conditional read, conditional create-if-absent,
conditional replacement, and stale-ETag rejection. Unsupported backends fail
closed for shared writes. Explicit single-process mode may use local
serialization but still writes every durable byte through OpenDAL.

Readers never lock. Writers may prepare immutable files concurrently. Visibility
changes only through Head. One Form table commit is the mutation atomicity unit;
Ugoite makes no cross-Form transaction claim. Leases, TTLs, heartbeats, lock
files, fences, independent metadata-history or commit engines, object-list
recovery, and custom maintenance engines are outside this architecture. The
logical-coordinate FileIO bridge accepts only canonical `ugoite://` locations,
binds them to the active Space operator, and rejects malformed or cross-Space
coordinates. A narrow physical-schema compatibility adapter is permitted only
when the upstream Rust API cannot retain an already-assigned Iceberg field ID;
it must produce standard upstream Iceberg metadata and cannot establish a
second field-identity authority.

## Space checkpoints

A `SpaceCheckpoint` is one reproducible Space-wide read coordinate, not a
transaction, authorization artifact, SQL request, or query policy. Capture
reads one exact Head, validates its reachable publication checksum and Head
checksum, then loads every referenced immutable Iceberg metadata file. It
records the Space ID, format/generation coordinates, publication location and
checksum, and each Form table's identifier, UUID, metadata location, snapshot
ID (when the table has one), and schema ID. A deterministic coordinate checksum
excludes optional capture time and human name.

Checkpoint reads never acquire a writer lock and never refresh from the current
Head. Each resolution rereads the checkpoint's immutable publication, verifies
its checksum and canonical Head, and requires that the Head's complete Form
table coordinates match the checkpoint before it reads Iceberg metadata. They
construct Iceberg static providers from the recorded metadata and, when
present, the recorded snapshot ID. A durable named checkpoint is created
once at `_ugoite/checkpoints/<name>.json` through OpenDAL; a missing durable
object or immutable target returns an explicit unavailable-checkpoint error.
There is no snapshot-expiration or custom retention implementation. If upstream
expiration is adopted later, durable checkpoints must first become retention
roots in that upstream design.

## Crate boundary

`ugoite-storage` owns normalized storage configuration, the Catalog Head
operator, capability probes, and the small conditional-object API. It does not
become a generic OpenDAL wrapper.

`ugoite-iceberg` owns the logical-coordinate FileIO bridge, `SpaceCatalog`,
physical schemas, typed Arrow conversion, upstream writers, table commits,
snapshots, DataFusion, checkpoints, and health evidence. Its physical upstream
types do not cross into Core.

## Read-only health evidence

The health endpoint starts from one exact Catalog Head, verifies its whole
reachable immutable publication chain, and reads only referenced Iceberg
metadata plus upstream manifest lists and manifests. It reports stable,
redacted issue codes, the Head checksum/generation/Form registry generation,
table identifiers/Form IDs/UUIDs/schemas/snapshots, and live data-file
count/record/size distribution evidence. Physical locations are redacted from
the normal API response.

Named checkpoints are caller-supplied and validated through immutable
publication, metadata, manifest, and data-file targets; health never lists
checkpoint storage. File evidence comes from upstream manifest entries, not a
metadata-table scan. Backend conditional-write capabilities and the durable
probe status are returned separately from unavailable orphan/failed-attempt
evidence. Failed-attempt and orphan candidates remain empty unless durable
attempt coordinates exist: object-list inference is forbidden.

`ugoite-core` owns domain validation, authorization meaning, use-case
orchestration, domain commands, receipts, checkpoints, and errors. It neither
constructs nor inspects OpenDAL, Iceberg, Arrow, Parquet, DataFusion, or SQL
parser objects.
