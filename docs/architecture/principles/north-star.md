---
title: "Architecture North Star"
sidebar:
  order: 2
---

This document defines the architectural promise, present boundary, target state,
and invariants that guide Ugoite development.

## Product promise

Ugoite is a private, portable knowledge space that users can run locally or on
infrastructure they control. A Space directory is the authoritative data
boundary; no hosted database is required to own or recover the content.

## Current state

- The Rust core reads and writes operator-owned Spaces.
- The CLI can call the core directly in local core mode.
- The Rust server exposes REST and a stateless semantic MCP facade and can serve
  the compiled browser application.
- The browser is currently server-backed and requires the server for Space
  persistence.
- The WASM crate exposes portable domain/API protocol logic, not a local storage
  engine.

## Persistence architecture

A Space is the complete durable boundary. It maps to one Iceberg namespace and
is reached only through the configured OpenDAL boundary. A server can expose a
Space, but it never owns a hidden catalog, relational database, or recovery
index for that Space.

That portable Space boundary is not the complete recovery boundary for a running
Node. Node accounts, sessions, credentials, and Node-to-Space bindings are
node-local control state below `_ugoite/nodes/{node-id}` by default, or in the
separate backend selected by `UGOITE_NODE_CONTROL_URI`. The encryption root
supplied by `UGOITE_NODE_SECRET_KEY` or `UGOITE_NODE_SECRET_FILE` is kept
outside that namespace. A complete Node recovery set therefore preserves the
Space prefix, the configured Node control-store prefix, and the node secret as
separate inputs. The default `/data` layout contains the two filesystem
prefixes, but a `/data` copy is complete only when the secret is preserved with
it too.

`_ugoite/catalog/head.json` is the only authoritative mutable catalog root. It
is published with an actual OpenDAL ETag compare-and-swap. Immutable,
checksum-protected publication records under `_ugoite/catalog/publications/`
link each successful Head generation to the preceding one. Iceberg metadata,
manifests, and data files remain Iceberg-owned immutable objects.

Readers pin immutable Head and snapshot coordinates and never lock. Writers can
prepare immutable objects concurrently, but make a mutation visible only by
replacing Head with the exact revision they read. An exact load returns bytes
and its revision from one object observation; a stat followed by an
unconditional read is not exact. The storage boundary admits a shared writer
only after a runtime behavioral probe proves create-if-absent, exact load,
conditional replacement, stale-revision rejection, and one winner for concurrent
CAS. This produces `SingleProcess`, `SharedReadOnly`, or `SharedVerified` write
mode based on the deployment contract and observed behavior, never on a provider
name.

`SharedVerified` uses the same immutable-before-publish plus one Head CAS
protocol as `SingleProcess`. Probe failure distinguishes unavailable exact reads
from readable-but-unverified conditional writes. Server timestamps are an
independent maintenance capability: they may gate age-based DerivedRelation GC,
but never authoritative Catalog or authorization writes. Catalog authority and
ordinary mutation correctness do not depend on leases, heartbeats, TTLs, lock
files, or fencing. DerivedRelation staging markers and special Space recovery
journals are lifecycle mechanisms only; they never establish Catalog visibility
or ordinary authorization.

## Target state

The browser should eventually open a local Space runtime and synchronize only
when the user enables an optional relay/collaboration service. The server should
not become the mandatory owner of user data.

## Invariants

1. Filesystem/object-storage-compatible Space content is the source of truth;
   the Space prefix is the portable move unit.
2. Revisions are append-only Iceberg table data; current state is derived from
   one table and recovery never depends on a hidden database.
3. Catalog Head is the only authoritative mutable root. Object listing never
   establishes catalog state or publication order.
4. Forms own authoritative history. DerivedRelation Heads own only replaceable
   current builds for indexes and projections; they never enter SpaceCheckpoint
   or authorization authority.
5. Domain and use-case behavior lives in reusable Rust crates.
6. CLI, server, WASM, and browser transport code remain adapters.
7. Authentication and authorization do not change ownership of Space data.
8. Current and planned capabilities are documented separately.
