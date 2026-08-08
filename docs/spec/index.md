---
title: "Ugoite specification index"
description: Executable specifications, requirements, interfaces, and implementation references for Ugoite.
sidebar:
  order: 1
---

**Updated:** 2026-06-29\
**Implementation status:** Rust/Deno v0.1 stream in progress

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

## Read the specification map

The specification is organized by the question it answers. Each group keeps its
existing source files and machine-readable registries; this page only makes the
reading order explicit.

- **Core model:** [data model](data-model/overview.md),
  [features](features/index.md), and the machine-readable
  [foundation](https://github.com/ugoite/ugoite/blob/main/docs/spec/philosophy/foundation.yaml).
- **Interfaces:** [REST API](api/rest.md),
  [OpenAPI](https://github.com/ugoite/ugoite/blob/main/docs/spec/api/openapi.yaml),
  [MCP](api/mcp.md), [operator surfaces](api/operator-surfaces.md), and
  [UI specifications](ui/index.md).
- **Requirements and stories:** [requirements](requirements/index.md) and
  [user stories](stories/index.md).
- **Architecture contracts:** [architecture overview](architecture/overview.md),
  decisions, stack, future-proofing, and the Space catalog.
- **Operations and quality:** [policies](policies/index.md),
  [security](security/overview.md), [testing and CI](testing/strategy.md),
  [quality](quality/error-handling.md),
  [product metrics](product/success-metrics.md), and
  [versions](versions/index.md).

Use the [operator guides](../guide/index.md) for procedures. This section is the
executable contract, not a beginner-first tutorial.

## Module matrix

| Module              | Responsibility                                                       |
| ------------------- | -------------------------------------------------------------------- |
| `ugoite-domain`     | portable domain types and validation                                 |
| `ugoite-api-client` | transport-neutral HTTP operation protocol                            |
| `ugoite-storage`    | OpenDAL-backed storage mechanics                                     |
| `ugoite-iceberg`    | Catalog-backed Form tables, batch append, query, and checkpoints     |
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
