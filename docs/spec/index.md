---
title: "Ugoite specification index"
description: Executable specifications, requirements, interfaces, and implementation references for Ugoite.
sidebar:
  order: 1
---

**Updated:** 2026-06-29\
**Implementation status:** Rust/Deno ver1 stream in progress

Ugoite is a local-first knowledge-space system based on three principles: **Low
Cost**, **Easy**, and **Freedom**. Operator-owned Space directories are
authoritative; indexes and query sessions are derived.

## Current boundary

- Local CLI core mode directly opens Spaces.
- The Rust server exposes REST, one read-only MCP resource, and static browser
  hosting.
- The browser is server-backed.
- Browser-local persistence and optional sync are planned.
- Passkey login, optional invited OIDC, opaque sessions, CLI device
  authorization, Agent Principals, Space ACLs, and authorization audit chains
  are operational.

## Navigation

- [Architecture](architecture/overview.md)
- [REST API](api/rest.md) and
  [OpenAPI](https://github.com/ugoite/ugoite/blob/main/docs/spec/api/openapi.yaml)
- [MCP](api/mcp.md) and [operator surfaces](api/operator-surfaces.md)
- [Data model](data-model/overview.md)
- [Feature registry](features/index.md)
- [Requirements](requirements/index.md)
- [UI specifications](ui/index.md)
- [Security](security/overview.md)
- [Testing and CI](testing/strategy.md)
- [Versions](versions/index.md)
- [Operator guides](../guide/concepts.md)

## Module matrix

| Module              | Responsibility                                                       |
| ------------------- | -------------------------------------------------------------------- |
| `ugoite-domain`     | portable domain types and validation                                 |
| `ugoite-api-client` | transport-neutral HTTP operation protocol                            |
| `ugoite-storage`    | OpenDAL-backed storage mechanics                                     |
| `ugoite-core`       | application service and persistence behavior                         |
| `ugoite-server`     | REST/MCP/auth/static-hosting adapter                                 |
| `ugoite-cli`        | local and remote command adapter                                     |
| `ugoite-wasm`       | JSON/C ABI over portable Rust crates                                 |
| `frontend`          | SolidStart UI and JavaScript fetch adapter                           |
| `docsite`           | Starlight build shell that renders the repository-level `docs/` tree |

## Sources of truth

- REST implementation and generated contract: `crates/ugoite-server` and
  `/openapi.json`.
- Portable remote-operation contract: `crates/ugoite-api-client`.
- Application behavior: `crates/ugoite-core`.
- Filesystem/object-storage behavior: `crates/ugoite-storage` plus core modules.
- Browser behavior: `frontend` (currently server-backed).
- Task and CI surface: root `mise.toml`, `deno.json`, and
  `.github/workflows/ci.yml`.

Machine-readable registries under `features/`, `requirements/`, `ui/`, and
`docs/version/` must reference existing source and test paths. Planned
capability must be labeled planned rather than represented as implemented.
