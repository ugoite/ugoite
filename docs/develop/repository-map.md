---
title: "Repository Map"
description: Where behavior lives and which crate owns it.
sidebar:
  order: 3
---

The repository map tells a contributor where to change behavior. Responsibility
follows
[CONTRIBUTING](https://github.com/ugoite/ugoite/blob/main/CONTRIBUTING.md) and
`AGENTS.md`: keep behavior in the smallest reusable layer and keep adapters
thin.

## Portable Rust layers

- `ugoite-domain`: pure domain types and validation, including WASM targets.
- `ugoite-api-client`: transport-neutral remote operation protocol with names,
  methods, paths, bodies, auth intent, and decoding. No network I/O, no fetch or
  reqwest dependency.
- `ugoite-storage`: storage abstraction and filesystem or object-store mechanics
  through OpenDAL.
- `ugoite-iceberg`: Catalog-backed Form tables, batch append, query, and
  publication Pins.
- `ugoite-core`: application service and persistence behavior.
- `ugoite-konase`: client-side Work and Job control semantics with serializable
  host effects. It does not own Knowledge or provider state.

## Thin adapters

- `ugoite-server`: REST, MCP, authentication, and static-hosting adapter. The
  server implementation in `crates/ugoite-server` and the generated contract at
  `/openapi.json` are the REST source of truth.
- `ugoite-cli`: local and remote command adapter. Full option detail lives in
  `ugoite <command> --help`, not in duplicated prose.
- `ugoite-wasm`: JSON and C ABI over the portable Rust crates. It owns no
  persistence, transport, or model runtime.
- `frontend`: SolidStart UI using the portable protocol. Browser API modules
  call the shared operation contract rather than constructing endpoint semantics
  directly.

## Supporting surfaces

- `docsite`: Astro and Starlight build shell that renders the repository-level
  `docs/` tree. Product prose is authored once under `docs/`.
- `e2e`, `tools`, `shared`, `charts`: validation, packaging, shared contracts,
  and deployment shapes driven by root `mise.toml` tasks.
- `docs/architecture`, `docs/spec`, `docs/mitase`, `docs/version`:
  implementation docs, executable specification, canonical Mitase graph, and
  version authority. See [Working with Specifications](../spec/index.md).

When a REST route changes, update the router and handler, update
`ugoite-api-client` when the operation is portable, update adapter and frontend
tests, regenerate OpenAPI, update the Mitase graph and human docs, then run
`mise run check`.
