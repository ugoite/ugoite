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

**Accepted.** A Space directory is the portable source of truth. Indexes,
projections, and SQL sessions are derived. Backups and migrations operate on the
complete directory tree.

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

## ADR-007 — MCP remains resource-first

**Accepted.** The current MCP surface is one authenticated, read-only Entry-list
resource. Tools and prompts remain future work.

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

## ADR-010 — Catalog and DataFusion are explicit

**Accepted.** REST Catalog is the multi-writer production Catalog. Local
single-process CLI mode uses the explicitly scoped MemoryCatalog plus its
portable pointer manifest; object listing must not infer metadata pointers.
MemoryCatalog is not used for shared production deployments.
DataFusion is the standard structured query engine and receives projection,
predicate, limit, join, and snapshot work through Iceberg providers.

Schema evolution receives a REST committer from the same typed configuration
used to construct the REST Catalog. The committer does not re-read a global
environment variable or construct a second endpoint configuration; Catalog
config response prefixes, headers, static credentials, and OAuth credential
exchange are applied to the atomic commit request. Iceberg Rust 0.8 is not
forked.

## ADR-011 — Portable logic is not a storage adapter

**Accepted.** `ugoite-domain` owns stable IDs, Form changes, compatibility,
revision construction, and I/O-free validation. WASM exposes that logic and the
portable API protocol; it never depends on Iceberg, Arrow, Parquet, OpenDAL, or
a native repository interface.
