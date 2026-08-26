---
title: 'Testing strategy'
---

Tests are organized around the shared Rust core and thin adapters.

- Rust unit/integration tests cover domain, storage, core, server, CLI, API protocol, and WASM.
- Frontend/docsite/tool tests run through Deno and Vitest.
- Frontend and docsite coverage run through the root
  `test:frontend:coverage` and `test:docsite:coverage` tasks, which are included
  by the canonical root `test` task. Hosted CI runs that aggregate task as a
  required hard gate with 100% V8 thresholds. The frontend unit gate targets the
  portable Rust/WASM protocol boundary; UI behavior remains under behavior tests
  and E2E. The docsite gate targets authored source. The active `main only pr`
  repository ruleset requires the `ci-required` status check.
- Playwright end-to-end tests exercise source/container parity.
- A focused docsite-navigation Playwright lane validates the built Starlight
  artifact, repository-level docs SSOT wiring, and GitHub Pages base-path
  navigation without paying the full backend/runtime-image startup cost.
- `xtask` checks OpenAPI drift, architectural dependency rules, and stale current-stack documentation.

Requirement IDs embedded in test names/source provide traceability. A requirement without a current test reference is labeled `untraced`; deleted paths are not retained as evidence.

The focused `tools/coverage_gates_test.ts` contract tests keep the package
coverage tasks, thresholds, canonical root quality graph, Hosted CI lane wiring,
required status aggregator, single-writer dependency/Deno caches, and
`REQ-OPS-021`/`REQ-OPS-024` traceability aligned. Hosted lanes may repack
semantic tasks for parallel runners, but they must not implement repository
validation independently of Mise.

Use focused crate/package tests while developing, then run `mise run ci` or the merge/release gates as appropriate.

For CLI-only work, start with `mise run test:cli` so you can iterate on `ugoite-cli` without running the full workspace suite.
