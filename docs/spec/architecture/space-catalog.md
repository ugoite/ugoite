---
title: 'SpaceCatalog publication contract'
---

This is the single durable-control-plane contract for a Ugoite Space. It is
intentionally destructive before the first release: the internal pre-release
Space format version is unchanged, while legacy layouts and catalog modes are
unsupported rather than migrated. The v0.1 compatibility floor freezes the
authority and history semantics described here; it does not permanently freeze
the current physical encoding.

The Catalog Head layout, upstream physical boundary, single mutation
coordinator, Pin-selected revision reads, authorization-aware DataFusion
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

The only mutable catalog root is currently encoded as
`_ugoite/catalog/head.json`. Immutable
publication records live at `_ugoite/catalog/publications/<generation>-<encoded-command-id>.json`.
Only records reachable from Head through `previous_publication` are authoritative.
Object listing never establishes current state or order.

DerivedRelation Heads under `_ugoite/derived/relations/{relation_id}/head.json`
are independent, non-authoritative pointers. They may be absent, stale, or
replaced without changing this Catalog Head or any pinned coordinate. Relation
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

## Changes, Runs, and Pins

Knowledge mutations attach a `ChangeDescriptor` to their immutable publication.
The command's `change_id` is the publication `command_id`; the descriptor records
the authenticated actor, optional message, optional history-only `RunId`, and an
optional `reverts_change_id`. `GET /spaces/{space_id}/changes` walks only the
reachable publication chain and returns committed Changes in chronological order.
There is no durable Run status or separate Change index in this history surface.
A future Undo operation is represented by new append-only publications whose
descriptors point at the Changes they selectively inverse.

## Asset visibility and retention

Asset deletion uses the same publication protocol. After checking current
references against the exact Head, the writer creates an immutable `asset.delete`
publication and makes it visible with the single Head CAS. Asset visibility is
derived by walking publications reachable from that Head; no command receipt or
Asset lifecycle sidecar is authoritative. Physical Asset bytes are retained in
v1, and automatic purge is not part of this protocol.

The Catalog Head owns the complete active Pin map. A Pin contains a
`PublicationRef` (generation, logical publication URI, and publication checksum),
plus creator and creation time. Creating or deleting a Pin is a metadata-only Head
publication; it does not create a user-content Change. Reads return the exact Head
map, and publication references are validated before use. Pin state is not
reconstructed by listing objects or by duplicating Space identity/checksum data.

## Capability and concurrency boundary

Shared writes require behavioral probes, not merely capability flags: a real
changing ETag, exact conditional read, conditional create-if-absent,
conditional replacement, stale-ETag rejection, and one winner when concurrent
writers use the same observed revision. A store starts in `SharedReadOnly` and
is promoted to `SharedVerified` only by this runtime probe; unsupported,
unavailable, and unverified stores fail closed for mutation permits. Server
timestamp availability is independent and only gates maintenance that needs
age comparisons. Explicit single-process mode may use local serialization but
still writes every durable byte through OpenDAL.

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

## PublicationRef and Pin selection

`PublicationRef` is the portable immutable read coordinate: generation, logical
publication URI, and publication checksum. The Catalog Head owns the complete
active Pin map, and each Pin stores exactly one `PublicationRef` plus creator
and creation time. Pin names are validated and bounded; a Pin is accepted only
when its coordinate is found with the same generation and checksum while
walking the publication chain reachable from the current Head.

Pin reads never acquire a writer lock or refresh from the current Head. Each
resolution rereads the exact Head, walks the reachable immutable publication
chain, verifies predecessor and checksum links, and loads the selected Head's
referenced Iceberg metadata. Missing, corrupt, or detached publications fail
closed as unavailable or integrity errors. The read implementation may build a
short-lived in-memory table-coordinate view for the Iceberg adapter, but it
never persists or treats that derived view as selection authority.

There is no snapshot-expiration or custom retention implementation. If upstream
expiration is adopted later, active Pins must first become retention roots in
that upstream design.

## Crate boundary

`ugoite-storage` owns normalized storage configuration, the Catalog Head
operator, capability probes, and the small conditional-object API. It does not
become a generic OpenDAL wrapper.

`ugoite-iceberg` owns the logical-coordinate FileIO bridge, `SpaceCatalog`,
physical schemas, typed Arrow conversion, upstream writers, table commits,
snapshots, publication-selected reads, DataFusion, and health evidence. Its physical upstream
types do not cross into Core.

## Read-only health evidence

The health endpoint starts from one exact Catalog Head, verifies its whole
reachable immutable publication chain, and reads only referenced Iceberg
metadata plus upstream manifest lists and manifests. It reports stable,
redacted issue codes, the Head checksum/generation/Form registry generation,
table identifiers/Form IDs/UUIDs/schemas/snapshots, and live data-file
count/record/size distribution evidence. Physical locations are redacted from
the normal API response.

Named checkpoints are caller-supplied and validated through the reachable immutable
publication chain, metadata, manifest, and data-file targets; health never lists
checkpoint storage. File evidence comes from upstream manifest entries, not a
metadata-table scan. Backend conditional-write capabilities and the durable
probe status are returned separately from unavailable orphan/failed-attempt
evidence. Failed-attempt and orphan candidates remain empty unless durable
attempt coordinates exist: object-list inference is forbidden.

`ugoite-core` owns domain validation, authorization meaning, use-case
orchestration, domain commands, Change/Revert planning, commit results,
publication coordinates, and errors. It neither
constructs nor inspects OpenDAL, Iceberg, Arrow, Parquet, DataFusion, or SQL
parser objects.
