---
title: 'Technology stack'
---

| Area | Current technology |
|---|---|
| Domain/core/storage/server/CLI/WASM | Rust 1.93 workspace |
| HTTP server | Axum |
| Storage abstraction | OpenDAL |
| Structured entry/revision tables | Apache Iceberg Rust integration |
| Local derived query/index work | SQLite-compatible query layer |
| Browser | SolidStart / TypeScript |
| Documentation site | Astro |
| JS/TS tooling | Deno 2.8 workspace |
| End-to-end tests | Playwright through Deno tasks |
| Task orchestration | mise |
| Container | one multi-stage image; non-root Debian runtime |
| CI | one GitHub Actions workflow invoking root mise gates |

There is no current parallel application backend or root alternate package-manager workflow.
