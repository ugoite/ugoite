---
title: "Testing strategy"
---

Tests are organized around the shared Rust core and thin adapters.

- Rust unit/integration tests cover domain, storage, core, server, CLI, API
  protocol, and WASM.
- Frontend/docsite/tool tests run through Deno and Vitest.
- Frontend coverage runs through the root `test:frontend:coverage` task and
  remains a required hard gate for the portable Rust/WASM protocol boundary.
  The canonical root `test` task and hosted web lane run the normal
  `test:docsite` behavior/type suite. `test:docsite:coverage` remains available
  as developer convenience for inspecting authored docsite source; its V8
  report has no hard threshold and is not a merge-quality metric. UI behavior
  remains under behavior tests and E2E. The active `main only pr` repository
  ruleset requires the `ci-required` status check.
- Playwright end-to-end tests exercise source/container parity.
- A focused docsite-navigation Playwright lane validates the built Starlight
  artifact, repository-level docs SSOT wiring, and GitHub Pages base-path
  navigation without paying the full backend/runtime-image startup cost.
- The v0.1 Knowledge compatibility fixture under
  `crates/ugoite-iceberg/tests/fixtures/` is exercised through the authoritative
  Space, publication, and history reader paths. It is a semantic oracle rather
  than a byte-for-byte snapshot of Iceberg internals.
- The older-CLI Knowledge preservation check runs the pinned v0.2.1 CLI against
  a Space after the current implementation saves a Composition. It verifies
  the older CLI can read and append unrelated Knowledge, then checks that
  reopening with the current implementation preserves both histories. It does
  not resolve, preview, execute, or rewrite the Composition with the older CLI.
  `mise run test:older-cli-knowledge-preservation` downloads and verifies the release
  manifest and platform archive digest. The hosted `ci:lane:rust-test` lane
  runs this check conditionally through its helper script; it is outside both
  `mise run test:rust` and the root `mise run test` task.
- `xtask` checks OpenAPI drift, architectural dependency rules, and stale
  current-stack documentation.
- Required acceptance lanes prove boundedness properties (paged reads with
  continuation, cancellation/retry, bounded memory, no implicit counts, ACL
  enforcement, conflict/unknown-version/broken-reference handling, portable
  reopen, and older-CLI Knowledge preservation) with small structural fixtures. Large
  fixed-count volumes are profiling-only; they are never merge gates and never
  release contracts.
- Seed profiling is instrumentation-only over the shared sample-data path.
  There are no profiling-only seed variants and therefore no separate
  profiled/unprofiled semantics oracle. Seed-phase timings are diagnostic, and
  seed micro-optimization stays out of the roadmap without
  product-bottleneck evidence from a user-facing path.

Requirement IDs embedded in test names/source provide traceability. A
requirement without a current test reference is labeled `untraced`; deleted
paths are not retained as evidence.

Focused checks cover required-result gate behavior, changed-path impact
selection, the frontend protocol coverage contract, and artifact provenance.
The hosted workflow owns lane execution; these tests do not freeze its job
layout, cache arrangement, or task ordering. Hosted lanes may package task
execution for their runners, but repository validation remains owned by the
root Mise tasks.

Use focused crate/package tests while developing, then run `mise run ci` or the
merge/release gates as appropriate.

Changes that can affect Space ownership, authority, publication reachability,
history reconstruction, or adapter authority must also carry the Knowledge
Compatibility Review required by the v0.1 compatibility floor.

For CLI-only work, start with `mise run test:smoke` so you can iterate on
`ugoite-cli` without running the full workspace suite.
