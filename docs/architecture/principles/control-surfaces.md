---
title: "Control surfaces"
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
| MCP        | one entry-list resource route | read-only, resource-first integration                                     |

Rules:

- Implement a use case once in core, then expose it through adapters.
- Keep path/method/body/decoding rules in `ugoite-api-client` when both CLI and
  browser use them.
- Keep credentials and runtime transport outside the portable crate.
- Do not infer feature completeness from a placeholder command or planned
  specification entry.
