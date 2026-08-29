---
title: "Architecture decisions"
---

These accepted decisions define the implementation boundaries that contributors
and adapters must preserve.

## ADR-001 — Rust is the canonical implementation

**Accepted.** Domain, storage, application behavior, REST, CLI, and WASM
protocol logic live in one Rust workspace. Adapters must not reimplement use
cases in another runtime.

## ADR-002 — Space files are authoritative

**Accepted.** A Space directory/prefix is the portable source of truth.
Indexes, projections, and SQL sessions are derived. Backups operate on the
complete Space prefix; pre-release format migrations are unsupported. Node
control state and the node secret are separate node-local recovery inputs.

## ADR-003 — Forms provide typed Markdown structure

**Accepted.** Entries remain Markdown-oriented while Forms define fields and
validation. Entry and revision records are stored through Form-specific
structured tables.

## ADR-004 — Remote operation semantics are portable

**Accepted.** `ugoite-api-client` owns operation names, method/path/body/auth
intent, and decoding. It performs no I/O. Native and browser runtimes supply
transport.

## ADR-005 — Current browser is server-backed

**Accepted current state.** JavaScript/WASM calls the Rust server. Browser-local
persistence and sync are a future runtime adapter, not an existing
implementation.

## ADR-006 — One release image

**Accepted.** The runtime image contains server, CLI, and static browser files,
runs as non-root, and mounts `/data`.

## ADR-007 — MCP is a small stable semantic facade

**Accepted.** MCP v1 exposes only `ugoite.search`, `ugoite.save`, and
`ugoite.delete`, filtered by authorization, plus lazy semantic Entry, history,
schema, and Form resources. It does not mirror REST endpoints or expose
storage/revision details; broader tools remain future work.

## ADR-008 — Iceberg-native workspace model

**Accepted.** A Space maps to one Iceberg namespace. A stable Form ID maps to
one physical `form_<uuid>` table; display-name changes never rename the table.
The Iceberg schema owns field IDs, types, and nullability. Versioned Ugoite
labels, validation, references, and semantic metadata live in table properties.

## ADR-009 — Entry history is the authority

**Accepted.** Create, update, delete, and restore append revision rows to the
Form table. Current state is the unique greatest `entry_version`; equal maximum
versions are a conflict. There is no authoritative current-entry table and no
two-table commit.

## ADR-010 — OpenDAL-published SpaceCatalog and DataFusion

**Accepted target.** A Space has one OpenDAL-backed `SpaceCatalog` implementing the
current Iceberg `Catalog` trait. Its sole mutable authority is
`_ugoite/catalog/head.json`; immutable linked publication records beneath
`_ugoite/catalog/publications/` provide the evidence needed to resolve an
ambiguous Head compare-and-swap after later writers publish.

Shared writes require a real Head ETag, exact `if_match` reads, conditional
initial creation, conditional whole-object replacement, stale rejection, and
one-winner concurrent CAS proven by backend probes. ETags are opaque tokens.
Non-local stores remain `SharedReadOnly` until the probe promotes them to
`SharedVerified`; unsupported or unverified stores fail closed. An explicitly
selected single-process mode may serialize in-process while retaining every
durable byte through OpenDAL. Readers never lock, and no lease, heartbeat,
TTL, lock file, fencing token, external Catalog, or relational database is
part of the production architecture.

Iceberg requirement/update application, filenames, and I/O use the current
official Iceberg Rust stack. Ugoite's logical-coordinate FileIO bridge accepts
only `ugoite://{space_uid}/{space-relative-key}`, resolves it against the
operator bound to the Space, and rejects malformed or cross-Space locations.
It never persists the physical warehouse URI or infers metadata from listings.
DataFusion is the standard structured query engine and receives
authorization-filtered, snapshot-pinned Iceberg providers. One Form-table
commit is atomic; cross-Form transactions are explicitly unsupported.

## ADR-011 — Portable logic is not a storage adapter

**Accepted.** `ugoite-domain` owns stable IDs, Form changes, compatibility,
revision construction, and I/O-free validation. WASM exposes that logic and the
portable API protocol; it never depends on Iceberg, Arrow, Parquet, OpenDAL, or
a native repository interface.

## ADR-012 — DerivedRelation is a sibling of Form

**Accepted.** A DerivedRelation is a typed Iceberg relation whose current
materialization can be deleted and rebuilt from authoritative Space data. Each
relation has an independent `_ugoite/derived/relations/{relation_id}/head.json`
published with OpenDAL conditional semantics. DerivedRelation updates never
change the main Catalog Head, Form revision history, Form registry generation,
or SpaceCheckpoint coordinates. AssetText is internal to trusted Quick Search
and is not exposed to Saved SQL, SQL Sessions, or authorization.

The first producer uses replace-all full rebuilds. Persistent substring
inverted indexes, cross-relation transactions, OCR, and a mandatory external
job queue are separate future work.

## ADR-013 - Konase is a client-side effect-driven control plane

**Accepted current slice.** ugoite-konase owns Work, Job, Observation,
Context Capsule, and deterministic Event/Effect transitions. It performs no
network, filesystem, storage, async runtime, or provider I/O. Native and WASM
hosts execute serializable effects and return results as events.

Konase state, agent memory, and raw model context are disposable client state.
Meaningful Knowledge outcomes continue through the existing Ugoite Space and
Change/Run/Undo semantics. A future View or Application Definition may be
Space-owned Knowledge, but its renderer and runtime state remain replaceable
Experience. Provider/framework implementations, Agent Plugins, MCP transport,
CLI UI, and browser UI remain replaceable adapters and are not part of this
slice.
