# Architecture decisions

## ADR-001 — Rust is the canonical implementation

**Accepted.** Domain, storage, application behavior, REST, CLI, and WASM protocol logic live in one Rust workspace. Adapters must not reimplement use cases in another runtime.

## ADR-002 — Space files are authoritative

**Accepted.** A Space directory is the portable source of truth. Indexes, projections, and SQL sessions are derived. Backups and migrations operate on the complete directory tree.

## ADR-003 — Forms provide typed Markdown structure

**Accepted.** Entries remain Markdown-oriented while Forms define fields and validation. Entry and revision records are stored through Form-specific structured tables.

## ADR-004 — Remote operation semantics are portable

**Accepted.** `ugoite-api-client` owns operation names, method/path/body/auth intent, and decoding. It performs no I/O. Native and browser runtimes supply transport.

## ADR-005 — Current browser is server-backed

**Accepted current state.** JavaScript/WASM calls the Rust server. Browser-local persistence and sync are a future runtime adapter, not an existing implementation.

## ADR-006 — One release image

**Accepted.** The runtime image contains server, CLI, and static browser files, runs as non-root, and mounts `/data`.

## ADR-007 — MCP remains resource-first

**Accepted.** The current MCP surface is one authenticated, read-only Entry-list resource. Tools and prompts remain future work.
