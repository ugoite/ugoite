---
title: "Cross-surface acceptance"
---

Informational. This document describes the acceptance model, not a new
specification authority. Outcome semantics stay with Mitase Requirement /
Criterion; the operation inventory stays with `UGOITE_API_OPERATIONS` and
its Rust mirror `SUPPORTED_OPERATIONS`.

## Principle: three surfaces, one meaning

Parity means reaching the same durable Knowledge outcome from different
surfaces, not building the same screen or the same command twice. The
Frontend may use Create and Edit dialogs while the CLI uses `form save`;
parity passes when the persisted Form carries equivalent schema semantics.

## Golden journey

`JOURNEY-KNOWLEDGE-001`: Space create -> Form establish -> Entry create ->
Entry edit -> Search -> History -> Restore.

Each checkpoint proves a durable postcondition, observable through the
canonical read surface after acting through any surface:

| Checkpoint | Postcondition |
| --- | --- |
| Space | A durable Space exists and reopens with identical compatibility semantics. |
| Form | Schema, required fields, and field-type interpretation match. |
| Entry create | An equivalent Knowledge object exists with exactly one new revision. |
| Entry edit | Optimistic concurrency and validation behave identically. |
| Search | The updated durable Entry is found under identical conditions. |
| History | Create and edit are observable as append-only history. |
| Restore | Restore appends a new revision or change; history never shortens. |

Business rules (validation, error classification, concurrency, history)
live in the shared Rust boundary. Fixtures supply values and observe
canonical representations; they never re-implement validation.

## Konase selected Context

`JOURNEY-KONASE-CONTEXT-001` is an acceptance journey for disposable Work
Context, not another Knowledge persistence journey. The shared portable Konase
engine defines the normalized `StartJob.context`; CLI and Browser Hosts acquire
only the explicitly selected Form or Entry resources with the current Space's
MCP credential and show the actual normalized Context before sending it to a
model. Browser REST Form lists and paged EntryQuery results are candidate
pickers, not authorization evidence. The selected resource is reread through
MCP. Denied, mismatched, invalid, or stale reads stop before model dispatch.

| Journey | Observable result | Evidence artifacts |
| --- | --- | --- |
| Explain the selected Form and Entry | Only those two URIs appear in the bounded portable Context, with untrusted-content framing; an unselected private Entry is absent. The shared `crates/ugoite-konase/fixtures/selected-context.json` input is consumed by the Rust portable, CLI, and Browser Host tests. | `crates/ugoite-konase/src/engine.rs`; `crates/ugoite-cli/src/commands/konase.rs`; `frontend/src/lib/konase/host.test.ts` |
| Read selection cannot be authorized or is stale | The Host stops before model dispatch when an MCP read is denied or mismatched; Space generation checks discard late Browser results. | `crates/ugoite-cli/src/commands/konase.rs`; `frontend/src/lib/konase/host.test.ts`; `frontend/src/components/konase/KonasePanel.test.tsx` |
| Start the existing Job without selected Context | The existing no-selection path starts one Job with an empty `selected_resource_contents` list. | `crates/ugoite-konase/src/engine.rs`; `crates/ugoite-wasm/src/lib.rs`; `frontend/src/lib/konase/host.test.ts` |
| A model proposes a Knowledge write | The separate KON-01 approval and validated mutation receipt remain required; only a confirmed receipt enables Work-scoped Undo. | `crates/ugoite-cli/src/commands/konase.rs`; `frontend/src/lib/konase/host.test.ts`; `crates/ugoite-server/src/lib.rs` |

These fixtures verify the portable resource projection and Host boundaries;
they do not golden-test model prose. Maximum resource count and byte/character
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
`deno run -A tools/capability_report.ts --scope v0.3-preflight --markdown`
(or `--json`). It locates declared source and evidence references; it does not
execute tests, prove authorization, or turn source presence into a passed
verification claim.
The report is informational in 0.1.x: gaps are diagnosed, not build
failures.

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
- C1: Golden journey outcome criteria (REQ-JOURNEY-001) with Frontend
  evidence (`e2e/knowledge-journey.test.ts`, 8 cases).
- C2: same scenario through the local/core CLI
  (`surface=cli, transport=core/local`).
- C3: same scenario through the server-backed CLI
  (`surface=cli, transport=remote`) with in-process server evidence.
  Remote Space creation stays out of scope by product design (browser
  session with recent Passkey plus node-admin role).
- C4: mutation semantic parity corpus (REQ-JOURNEY-002) on both CLI
  transports: validation, conflict, authorization, and history meaning.
  Presentation wording stays surface-owned.
- C5: delete parity (tombstone core positive, remote approval guard) and
  Frontend-only usability criteria (REQ-JOURNEY-003) with no CLI parity.
- C6: stable executed corpus qualifies release-candidate creation via
  `qualifyAcceptanceCorpus` in `tools/release.ts`. The Playwright journey
  stays on the `full` E2E lane until the v0.2 closure.
- C7 was planned for v0.2 but was not completed before v0.2.0 publication.
  Issue [#2563](https://github.com/ugoite/ugoite/issues/2563) remains the
  independent acceptance-gate implementation follow-up; the horizon wording is
  tracked by [#3123](https://github.com/ugoite/ugoite/issues/3123). The v0.3
  preflight documents major gaps and their owners but does not activate a
  release-blocking gate or decide its acceptance policy.

Mitase never executes tests; it declares which exact implementation and
verification targets prove a criterion, and the runner proves they pass.
