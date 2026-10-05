---
title: "Control surfaces"
sidebar:
  order: 4
---

Ugoite exposes one application model through several adapters.

| Surface    | Current implementation        | Boundary                                                                  |
| ---------- | ----------------------------- | ------------------------------------------------------------------------- |
| Core API   | `crates/ugoite-core`          | canonical use cases over Spaces                                           |
| Local CLI  | `ugoite` in core mode         | direct local workspace access                                             |
| Remote CLI | `ugoite` in backend/API mode  | portable operation protocol + native HTTP transport                       |
| REST       | `ugoite-server`               | authenticated/authorized HTTP adapter                                     |
| Browser    | SolidStart frontend           | portable operation protocol + JavaScript `fetch`; currently server-backed |
| WASM       | `ugoite-wasm`                 | JSON/C ABI over portable Rust crates; no persistence/transport            |
| MCP        | stateless `/mcp` facade with three filtered tools and lazy resources | stable semantic integration; breadth is cheap and depth is lazy |

Rules:

- Implement a use case once in core, then expose it through adapters.
- Keep path/method/body/decoding rules in `ugoite-api-client` when both CLI and
  browser use them.
- Keep credentials and runtime transport outside the portable crate.
- Do not infer feature completeness from a placeholder command or planned
  specification entry.

## Facet vocabulary

Mitase treats binding facets as opaque strings; the meaning below is owned
by Ugoite, not by Mitase.

| Facet | Meaning | Notes |
| ----- | ------- | ----- |
| `core` | canonical durable/domain implementation | strongest semantic owner |
| `service` | application/service boundary | support boundary, not necessarily a user surface |
| `backend` | REST/API server implementation | executed server code |
| `frontend` | browser/frontend client implementation | API module or UI behavior |
| `cli-core` | local workspace direct CLI implementation | only with an exact transport-specific target |
| `cli-remote` | backend/API-routed CLI implementation | only with an exact transport-specific target |
| `cli` | shared CLI implementation | transitional, until core/remote split into exact targets |
| `mcp` | MCP semantic facade implementation | a small facade, never a REST mirror |
| `client-host` | Konase Host boundary | current naming |
| `wasm` | WASM portable adapter | only when semantically distinct |
| `api` | OpenAPI / contract source | `role: contract-source` only |
| `verification` | verification binding | not a runtime surface |

`backend` is executed server implementation; `api` is the authoritative
HTTP contract source. Never confuse the two.
