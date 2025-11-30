# IEapp Implementation Tasks & TDD Plan

## Guiding Principles
- **Small Start**: Each milestone is a thin vertical slice that unlocks an immediately usable capability before layering on extras.
- **Spec Mapping**: Every task references the relevant spec sections (00-05) to ensure alignment.
- **Strict TDD**: Write failing tests first (pytest/bun/playwright) and keep the green cycle tight before adding new behavior.
- **Automation First**: Ruff/Biome formatters and CI commands (`pytest`, `bun test`, `playwright test`) must run in each milestone before it is declared complete.

## Milestone 0 — Workspace Skeleton & Local FS (Specs: 01 §Architecture, 03 §Storage, 05 §Testing)
- **Goal**: Single-workspace lifecycle on local disk using `fsspec`; verifies `global.json` + `{workspace}/meta.json` scaffolding.
- **Stories/FRs covered**: Base for Story 3, FR-01.x, Security defaults from 05.
- **TDD Steps**:
  1. Write pytest verifying `create_workspace()` generates files per 03 §2 (use tmp path + checksum assertions).
  2. Add pytest for `fsspec` adapter fallback (local path + fake S3 URI raising unimplemented error).
  3. Only after tests exist, implement minimal `ieapp.workspace` module + CLI entry.
- **Implementation Notes**:
  - Enforce chmod 600 on created dirs (05 §1).
  - Emit structured logging stub (05 §3) even if sink is noop.

## Milestone 1 — Note Storage & History (Specs: 02 Story 5, 03 §5-7, 05 §3)
- **Goal**: Append-only note directories with `meta.json`, `content.json`, `history/index.json`.
- **TDD Steps**:
  1. Pytest: writing a note requires `parent_revision_id`; expect 409 on mismatch (simulate with fixture).
  2. Pytest: Markdown parsed into headers & frontmatter and persisted to `content.json`.
  3. Pytest: History append updates index + checksum & signature fields (mock HMAC provider).
- **Implementation Notes**:
  - Reuse parser logic for later indexer; keep API-free for now (pure library).
  - Provide CLI smoke command (`ieapp note create`) that reuses library to get fast feedback.

## Milestone 2 — Live Indexer & Query Surface (Specs: 01 §Structure-from-Text, 02 Story 2, 03 §3-5)
- **Goal**: Background indexer that watches note changes and projects into `index/index.json`, `stats.json`.
- **TDD Steps**:
  1. Pytest: given Markdown with multiple H2 sections, indexer emits `properties` dict with precedence rules (03 §3 table).
  2. Pytest: class validation raises structured warnings when required headers missing.
  3. Pytest: stats aggregator reports counts per class.
- **Implementation Notes**:
  - Implement watch loop as injectable dependency so tests drive it synchronously.
  - Provide simple query function `ieapp.query(filter=...)` using cached index for later API reuse.

## Milestone 3 — Minimal FastAPI REST (Specs: 04 §REST, 05 §Security)
- **Goal**: REST endpoints for workspaces + notes using library primitives; still local-only.
- **TDD Steps**:
  1. FastAPI TestClient tests for `POST /workspaces` and `POST /workspaces/{ws}/notes` (happy path + 409 conflict).
  2. Test for structured query endpoint returning indexed properties.
  3. Test middleware enforces localhost binding & HMAC header injection on responses.
- **Implementation Notes**:
  - Wire dependency injection so API uses `ieapp` services; avoid duplicating logic.
  - Document OpenAPI examples for canvas fields even if not yet editable (preparing Story 4).

## Milestone 4 — MCP Resources & Wasm Sandbox (Specs: 01 §Code Execution, 02 Story 1 & 8, 04 §MCP, 05 §Sandbox)
- **Goal**: MCP endpoint exposing `run_script` tool with Wasm/JavaScript sandbox guardrails.
- **TDD Steps**:
  1. Pytest: sandbox denies direct filesystem/network access (verify Wasm isolation).
  2. Pytest/integration: `run_script` can execute JavaScript that calls `host.call` to query and update notes (mocked workspace + deterministic script).
  3. Contract test: MCP resource serialization matches spec (snapshot expected JSON).
  4. Pytest: verify fuel limits prevent infinite loops (script exceeding fuel budget returns error).
- **Implementation Notes**:
  - Use `wasmtime` Python bindings with a pre-compiled JavaScript engine (e.g., QuickJS via Javy).
  - Implement `host.call` as a Wasm import that proxies to internal REST API handlers.
  - Dynamically expose available API operations based on OpenAPI spec.
  - Reuse REST auth toggles for MCP (shared dependency config).

## Milestone 5 — Frontend Thin Slice (Specs: 02 Story 2 & 4, 04 REST usage)
- **Goal**: Solid Start app with login-less local mode, list view, markdown editor pushing to REST.
- **TDD Steps**:
  1. Component tests (bun test) for note list store interacting with REST mocks.
  2. Playwright smoke: create note → see header extracted (uses index query stub).
  3. Visual regression baseline for canvas placeholder (even if static positions for now).
- **Implementation Notes**:
  - Use optimistic updates but reconcile with server revision_id (Story 2 acceptance).
  - Start with list view before canvas interactions (small start on Story 4).

## Milestone 6 — Search, Attachments, Canvas Links (Specs: 02 Stories 3-6, 03 §2 attachments, 04 search/links)
- **Goal**: Complete user stories around search, attachments, canvas linking, BYO storage connectors.
- **TDD Steps**:
  1. Pytest: FAISS + inverted index integration (use tiny embedding fixture).
  2. API tests: attachments upload + garbage collection guard.
  3. Frontend Playwright: drag-drop link creation persists via REST.
- **Implementation Notes**:
  - Introduce background worker for embedding generation (mock in tests).
  - Provide storage connector validation route tests for S3 + local (Story 3).

## Milestone 7 — Observability, CI, Hardening (Specs: 05 §Security/Testing)
- **Goal**: Production-ready posture (CI pipelines, lint/type, logging, error handling scenarios).
- **TDD Steps**:
  1. Add Schemathesis contract run in CI (fails until spec + API align).
  2. Pytest: simulate storage outage returning 503 with retry headers.
  3. Security tests: ensure auth required when binding to non-loopback.
- **Implementation Notes**:
  - Wire Ruff/Biome/Playwright commands into CI pipeline.
  - Document incident playbooks in README + `docs/spec/05_security_and_quality.md` cross-reference.

## Ongoing TDD Rhythm
- Start each work item by writing/adjusting tests in the layer closest to the spec requirement.
- Keep fixtures focused: prefer factory helpers in `tests/factories.py` for notes/workspaces.
- When adding a new feature, extend docs/spec with decisions before coding to keep spec as the source of truth.
