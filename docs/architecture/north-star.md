# Architecture North Star

## Product promise

Ugoite is a private, portable knowledge space that users can run locally or on
infrastructure they control. A Space directory is the authoritative data
boundary; no hosted database is required to own or recover the content.

## Current state

- The Rust core reads and writes operator-owned Spaces.
- The CLI can call the core directly in local core mode.
- The Rust server exposes REST and a resource-first MCP endpoint and can serve
  the compiled browser application.
- The browser is currently server-backed and requires the server for Space
  persistence.
- The WASM crate exposes portable domain/API protocol logic, not a local storage
  engine.

## Target state

The browser should eventually open a local Space runtime and synchronize only
when the user enables an optional relay/collaboration service. The server should
not become the mandatory owner of user data.

## Invariants

1. Filesystem/object-storage-compatible Space content is the source of truth.
2. Revisions are append-only; recovery must not depend on a hidden database.
3. Indexes, projections, and query sessions are derived and replaceable.
4. Domain and use-case behavior lives in reusable Rust crates.
5. CLI, server, WASM, and browser transport code remain adapters.
6. Authentication and authorization do not change ownership of Space data.
7. Current and planned capabilities are documented separately.
