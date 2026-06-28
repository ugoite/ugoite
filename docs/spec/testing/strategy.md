---
title: 'Testing strategy'
---

Tests are organized around the shared Rust core and thin adapters.

- Rust unit/integration tests cover domain, storage, core, server, CLI, API protocol, and WASM.
- Frontend/docsite/tool tests run through Deno and Vitest.
- Playwright end-to-end tests exercise source/container parity.
- A focused docsite-navigation Playwright lane validates the built Starlight
  artifact, repository-level docs SSOT wiring, and GitHub Pages base-path
  navigation without paying the full backend/runtime-image startup cost.
- `xtask` checks OpenAPI drift, architectural dependency rules, and stale current-stack documentation.

Requirement IDs embedded in test names/source provide traceability. A requirement without a current test reference is labeled `untraced`; deleted paths are not retained as evidence.

Use focused crate/package tests while developing, then run `mise run ci` or the merge/release gates as appropriate.

For CLI-only work, start with `mise run test:cli` so you can iterate on `ugoite-cli` without running the full workspace suite.
