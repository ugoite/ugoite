---
title: "Cross-surface acceptance"
---

Informational. This document describes the acceptance model, not a new
specification authority. Outcome semantics stay with Mitase Requirement /
Criterion; the operation inventory stays with `UGOITE_API_OPERATIONS` and its
Rust mirror `SUPPORTED_OPERATIONS`.

## Principle: three surfaces, one meaning

Parity means reaching the same durable Knowledge outcome from different
surfaces, not building the same screen or the same command twice. The Frontend
may use Create and Edit dialogs while the CLI uses `form save`; parity passes
when the persisted Form carries equivalent schema semantics.

## Golden journey

`JOURNEY-KNOWLEDGE-001`: Space create -> Form establish -> Entry create -> Entry
edit -> Search -> History -> Restore.

Each checkpoint proves a durable postcondition, observable through the canonical
read surface after acting through any surface:

| Checkpoint   | Postcondition                                                              |
| ------------ | -------------------------------------------------------------------------- |
| Space        | A durable Space exists and reopens with identical compatibility semantics. |
| Form         | Schema, required fields, and field-type interpretation match.              |
| Entry create | An equivalent Knowledge object exists with exactly one new revision.       |
| Entry edit   | Optimistic concurrency and validation behave identically.                  |
| Search       | The updated durable Entry is found under identical conditions.             |
| History      | Create and edit are observable as append-only history.                     |
| Restore      | Restore appends a new revision or change; history never shortens.          |

Business rules (validation, error classification, concurrency, history) live in
the shared Rust boundary. Fixtures supply values and observe canonical
representations; they never re-implement validation.

## Product journeys for the v0.2.1 candidate

This matrix fixes the user-visible completion condition and evidence boundary
for the seven journeys in the v0.2.1 implementation plan. A test selector is a
locator, not proof that a run passed. Record each run against its exact source
SHA and keep mock/unit evidence separate from real-server acceptance.

| Journey | Starting state and operation                                                                                                                                | Observable done                                                                                                                                                        | Recovery or error result                                                                                                                                                         | Current evidence and limit                                                                                                                                                                                                                                                                                                   |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entry   | An open Space and a Form with declared fields; create, edit, find, and restore an Entry.                                                                    | The receipt identity reads back; a stale parent is rejected; search finds the saved Entry; restore appends a revision without shortening history.                      | Re-read the current revision after conflict; validation failure leaves the prior revision intact; confirm restore from History.                                                  | `e2e/knowledge-journey.test.ts` (`JOURNEY-KNOWLEDGE-001`, `JOURNEY-LOCATE-RECOVER-001`) proves postconditions through the test server API. `e2e/entries.test.ts` covers Browser behavior. The same end-to-end operation through local and remote CLI is not yet evidenced.                                                   |
| Form    | An open Space; define or evolve a Form, save it, and read it back.                                                                                          | Name, version, required fields, and field types match; a no-op is observable as no new durable change.                                                                 | Invalid schema is rejected with the existing Form intact; after an uncertain write, read the Form before retrying.                                                               | `e2e/forms.test.ts` covers server creation/listing and immediate queryability. `e2e/knowledge-journey.test.ts` checks persisted schema semantics. Operation-owned Change receipt and no-op coverage must be recorded separately.                                                                                             |
| Asset   | An open Space and a Form with an Asset field; upload an Asset, attach its reference, and reopen the Entry.                                                  | The reference belongs to the intended Form, resolves to the uploaded bytes, and reads back from the Entry.                                                             | A rejected upload or invalid reference leaves the prior Entry state readable; check the upload receipt and current Entry before retrying.                                        | `e2e/entries.test.ts` (`REQ-FE-1877`, `@asset-owned`) covers independent Form ownership in the test environment. It does not prove every upload receipt or copy/recovery path.                                                                                                                                               |
| Search  | An open Space containing a known Entry; search by keyword and declared typed fields.                                                                        | The matching Entry identity and requested fields appear under the same query conditions after save and reopen.                                                         | Empty results remain distinguishable from query failure; correct the query or field scope and rerun without changing the Entry.                                                  | `e2e/search-ui.test.ts` (`REQ-SRCH-004`, `REQ-SRCH-006`) covers Browser keyword discovery; `e2e/knowledge-journey.test.ts` covers server query postconditions. CLI parity and cancellation at the server execution boundary are separate evidence.                                                                           |
| SQL     | An open Space with a queryable Form; save a query, run it, and export its result where supported.                                                           | Saved SQL reads back with its revision; the run shows expected columns and rows; export reports completion only after all pages are written.                           | Query/auth errors stay visible; partial stdout is nonzero and incomplete, while file output is discarded on failure. Reauthorize before retrying expired or revoked credentials. | `e2e/saved-sql-route.test.ts` (`REQ-FE-061`–`063`) covers Browser saved-query navigation and routing. `e2e/sql-export-remote-auth.test.ts` covers remote authorization changes during export. These selectors do not prove bounded bytes, every continuation failure, or a completed Browser result journey.                 |
| History | An Entry or Space with an appended Change; inspect history, compare revisions, and restore/revert when authorized.                                          | Prior content is recovered by a new revision or inverse Change; earlier history remains observable.                                                                    | A stale or unauthorized mutation is rejected without changing history; reread the current head before another attempt.                                                           | `e2e/knowledge-journey.test.ts` covers Entry restore and Space Change revert. `e2e/entries.test.ts` (`@smoke History closes a confirmed revert and records the appended Change`) covers the Browser confirmation path. Retaining confirmed success after refresh failure requires its own evidence.                          |
| Konase  | An authorized Space and user-selected Context; inspect the exact Context, request a proposed Knowledge change, approve it, and observe the mutation result. | Only selected, authorized resources enter the request; a write is reported saved only with a valid confirmation receipt; Undo is available only for a confirmed write. | Denied, revoked, mismatched, or stale reads stop before model dispatch; invalidated pending approval fails closed; check an unknown write before retrying.                       | Portable, CLI, and Browser Host selectors are declared under `crates/ugoite-konase`, `crates/ugoite-cli`, and `frontend/src/lib/konase/host.test.ts`. These are not a cross-surface real-server proof. Real partial-save and revocation acceptance remains tracked by [#3157](https://github.com/ugoite/ugoite/issues/3157). |

### Runnable selector index

These commands locate the current tests; a completed run still needs an evidence
record with its source SHA, environment, result, and limits.

| Journey | Command and selector                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entry   | `deno task --cwd e2e knowledge-journey` runs `JOURNEY-KNOWLEDGE-001` and `JOURNEY-LOCATE-RECOVER-001` in `e2e/knowledge-journey.test.ts`.                                                                                                                                                                                                                                                                                                                                                       |
| Form    | `deno task --cwd e2e forms` runs `Create and List Forms`, `Query Entries by Form`, and `Issue 2138: a newly created Form is immediately queryable` in `e2e/forms.test.ts`.                                                                                                                                                                                                                                                                                                                      |
| Asset   | `deno task --cwd e2e asset-owned` runs `REQ-FE-1877` in `e2e/entries.test.ts`.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Search  | From `e2e/`, `deno run --node-modules-dir=none -A npm:@playwright/test@^1.60.0 test search-ui.test.ts --grep REQ-SRCH-004` runs the Browser keyword-search case. The navigation case runs with `--grep REQ-SRCH-006`.                                                                                                                                                                                                                                                                           |
| SQL     | From `e2e/`, `deno run --node-modules-dir=none -A npm:@playwright/test@^1.60.0 test saved-sql-route.test.ts` runs `REQ-FE-061` through `REQ-FE-063`. `deno task --cwd e2e sql-export-remote-auth` runs `a live membership revoke rejects page two and leaves no output` and `refreshes a credential that expires between SQL pages` in `e2e/sql-export-remote-auth.test.ts`.                                                                                                                    |
| History | `deno task --cwd e2e knowledge-journey` runs append-only restore/revert cases. `deno run --node-modules-dir=none -A npm:@playwright/test@^1.60.0 test entries.test.ts --grep "History closes a confirmed revert and records the appended Change"` from `e2e/` runs the Browser History case.                                                                                                                                                                                                    |
| Konase  | Run `cargo test -p ugoite-konase selected_resources_are_normalized_into_start_context_only --locked`, `cargo test -p ugoite-cli selected_resource_is_read_before_model_and_normalized_context_is_sent --locked`, and `cargo test -p ugoite-cli denied_or_mismatched_selected_read_stops_before_model_call --locked`; run Browser Host tests with `deno task frontend:test 'src/lib/konase/host.test.ts'`. These are portable, CLI, and Browser Host unit selectors, not real-server acceptance. |

### Run evidence record

For each executed selector, record the exact candidate source SHA, fixture or
seed, command and selector, surface and transport, environment, expected and
actual result, exit code, artifact location, and unverified scope. A PR check
summary may link the CI run, but it does not replace the selector-level result.
Do not label source inspection, mocked responses, or static capability reports
as real-server or Browser passes. When a journey uses several surfaces, record
each surface independently; a pass on one surface does not imply parity.

## Konase selected Context

`JOURNEY-KONASE-CONTEXT-001` is an acceptance journey for disposable Work
Context, not another Knowledge persistence journey. The shared portable Konase
engine defines the normalized `StartJob.context`; CLI and Browser Hosts acquire
only the explicitly selected Form or Entry resources with the current Space's
MCP credential and show the actual normalized Context before sending it to a
model. Browser REST Form lists and paged EntryQuery results are candidate
pickers, not authorization evidence. The selected resource is reread through
MCP. Denied, mismatched, invalid, or stale reads stop before model dispatch.

| Journey                                         | Observable result                                                                                                                                                                                                                                                                                                                                                        | Evidence artifacts                                                                                                                                                                                     |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Explain the selected Form and Entry             | Only those two URIs appear in the bounded portable Context, with untrusted-content framing; an unselected private Entry is absent. The shared `crates/ugoite-konase/fixtures/selected-context.json` input is consumed by the Rust portable, CLI, and Browser Host tests.                                                                                                 | `crates/ugoite-konase/src/engine.rs`; `crates/ugoite-cli/src/commands/konase.rs`; `frontend/src/lib/konase/host.test.ts`                                                                               |
| Report selected-resource admission accurately   | Only a parsed top-level `_ugoite_context.truncated: true` marks a compacted projection; an Entry body containing marker-like text remains included. Four supplied selection values are accepted and deduplicated, while five values are rejected even when their URIs repeat. The Rig initial model request includes selected Context and stays within 8,192 characters. | `crates/ugoite-konase/src/context.rs`; `crates/ugoite-konase/src/engine.rs`; `crates/ugoite-konase-rig/src/lib.rs`; `crates/ugoite-cli/src/commands/konase.rs`; `frontend/src/lib/konase/host.test.ts` |
| Read selection cannot be authorized or is stale | The Host stops before model dispatch when an MCP read is denied or mismatched; Space generation checks discard late Browser results.                                                                                                                                                                                                                                     | `crates/ugoite-cli/src/commands/konase.rs`; `frontend/src/lib/konase/host.test.ts`; `frontend/src/components/konase/KonasePanel.test.tsx`                                                              |
| Start the existing Job without selected Context | The existing no-selection path starts one Job with an empty `selected_resource_contents` list.                                                                                                                                                                                                                                                                           | `crates/ugoite-konase/src/engine.rs`; `crates/ugoite-wasm/src/lib.rs`; `frontend/src/lib/konase/host.test.ts`                                                                                          |
| A model proposes a Knowledge write              | The separate KON-01 approval and validated mutation receipt remain required; only a confirmed receipt enables Work-scoped Undo.                                                                                                                                                                                                                                          | `crates/ugoite-cli/src/commands/konase.rs`; `frontend/src/lib/konase/host.test.ts`; `crates/ugoite-server/src/lib.rs`                                                                                  |

These fixtures verify the portable resource projection and Host boundaries; they
do not golden-test model prose. Maximum resource count and byte/character
budgets remain owned by the portable Context implementation. Static references
to these selectors are declared in Mitase under `REQ-API-012`; the
`v0.3-preflight` capability report remains a static locator and does not claim
that tests ran or that the resource was authorized.

Entry creation uses the identity returned in its mutation receipt for later
operations. Form save and `sql saved` are CLI input models for the same durable
Form and Saved SQL outcomes; their command spelling does not define separate
business semantics.

## Capability projection (generated, not authoritative)

`tools/capability_report.ts` projects the journey from existing authorities:

- Inventory: `UGOITE_API_OPERATIONS` + `SUPPORTED_OPERATIONS` (must match).
- Surface usage: Frontend `*-api.ts`, CLI remote `http::execute`, CLI core
  `UgoiteService` methods.
- Verification: exact e2e test names plus exact Mitase `verifies` claims.

Run `deno run -A tools/capability_report.ts --markdown` (or `--json`). The
separate v0.3 planning inventory is generated with
`deno run -A tools/capability_report.ts --scope v0.3-preflight --markdown` (or
`--json`). It locates declared source and evidence references; it does not
execute tests, prove authorization, or turn source presence into a passed
verification claim. The report is informational in 0.1.x: gaps are diagnosed,
not build failures.

Row states:

- `verified`: every surface reaches the outcome with exact evidence.
- `evidence-gap`: reachable, but e2e or Mitase evidence is missing.
- `surface-gap`: at least one surface cannot reach the outcome.
- `semantic-drift`: adapters disagree on inventory or shared encoding.
- `intentionally-not-required`: reserved for obligations scoped to fewer
  surfaces (for example Frontend-only UX polish). No journey row uses it.

Classification priority is semantic-drift > surface-gap > evidence-gap >
verified, so missing reachability remains distinct from missing evidence.

## Status and next steps

- C0: informational projection (`tools/capability_report.ts`).
- C1: Golden journey outcome criteria (REQ-JOURNEY-001) with Frontend evidence
  (`e2e/knowledge-journey.test.ts`, 8 cases).
- C2: same scenario through the local/core CLI
  (`surface=cli, transport=core/local`).
- C3: same scenario through the server-backed CLI
  (`surface=cli, transport=remote`) with in-process server evidence. Remote
  Space creation stays out of scope by product design (browser session with
  recent Passkey plus node-admin role).
- C4: mutation semantic parity corpus (REQ-JOURNEY-002) on both CLI transports:
  validation, conflict, authorization, and history meaning. Presentation wording
  stays surface-owned.
- C5: delete parity (tombstone core positive, remote approval guard) and
  Frontend-only usability criteria (REQ-JOURNEY-003) with no CLI parity.
- C6: stable executed corpus qualifies release-candidate creation via
  `qualifyAcceptanceCorpus` in `tools/release.ts`. The Playwright journey stays
  on the `full` E2E lane until the v0.2 closure.
- C7 was planned for v0.2 but was not completed before v0.2.0 publication. Issue
  [#2563](https://github.com/ugoite/ugoite/issues/2563) remains the independent
  acceptance-gate implementation follow-up; the horizon wording is tracked by
  [#3123](https://github.com/ugoite/ugoite/issues/3123). The v0.3 preflight
  documents major gaps and their owners but does not activate a release-blocking
  gate or decide its acceptance policy.

Mitase never executes tests; it declares which exact implementation and
verification targets prove a criterion, and the runner proves they pass.

The v0.2.1 journey matrix is an acceptance definition and evidence index, not a
claim that all seven journeys or all listed surfaces have passed. Final evidence
requires the feature lanes to merge and the relevant selectors to run against
the exact candidate SHA; that execution record is tracked in
[#3240](https://github.com/ugoite/ugoite/issues/3240).
