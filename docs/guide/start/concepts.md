---
title: "Core concepts"
---

A **Space** is Ugoite's ownership and storage boundary. On disk it is a
directory below the configured root containing entries, forms, assets, saved
SQL, history, membership data, and derived indexes.

The Space directory is authoritative. Indexes and query sessions are derived and
replaceable.

An **Entry** is a versioned JSON document. A **Form** describes Entry fields.
Updates and restores create append-only revisions rather than rewriting old
history.

## Runtime modes

- **Core mode:** the CLI calls `ugoite-core` directly against a local workspace.
- **Backend/API mode:** the CLI or browser calls the Rust HTTP server, which
  authenticates, authorizes, and invokes the same core service.
- **Browser:** currently server-backed. Browser-local persistence and optional
  sync are planned, not shipped.

Domain and use-case behavior belongs in Rust core crates; CLI, REST, MCP, WASM,
and browser transports are adapters.
