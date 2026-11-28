---
title: "Cross-surface acceptance"
---

This page explains the acceptance model and the generated capability report. It
does not define product requirements or serve as a run-status ledger. Mitase
owns current journey criteria and their verification bindings; the portable API
registry owns the published operation inventory.

## One Knowledge outcome across surfaces

Parity means reaching the same durable Knowledge outcome from different
surfaces, not building the same screen or command twice. The Frontend may use
Create and Edit dialogs while the CLI uses `form save`; parity passes when the
persisted Form carries equivalent schema semantics.

`JOURNEY-KNOWLEDGE-001` covers Space create -> Form establish -> Entry create ->
Entry edit -> Search -> History -> Restore. Its key postconditions are:

- a Space reopens with identical compatibility semantics;
- Form schema and field interpretation are consistent;
- create and edit append the expected Knowledge revisions;
- validation and optimistic concurrency have equivalent outcomes;
- search observes the updated durable Entry; and
- restore appends history without shortening it.

The owning criteria and exact verification selectors are in
`docs/mitase/requirements/journey.yaml`. Business rules for validation, error
classification, concurrency, and history belong to the shared Rust boundary.
Surface tests verify their transport and interaction boundaries.

## Capability projection

`tools/capability_report.ts` projects existing operation, surface, Requirement,
and verification declarations. Run it from the repository root with:

```sh
deno run -A tools/capability_report.ts --markdown
deno run -A tools/capability_report.ts --scope v0.3-preflight --markdown
```

The default report describes the Knowledge journey. The preflight scope locates
declared source and evidence references for the planned capability set. Neither
report executes tests or proves authorization. A source path or selector is a
locator, not evidence that a check ran; exact execution results belong to the
relevant pull request or Actions run.

Report states distinguish verified coverage from evidence gaps, surface gaps,
semantic drift, and intentionally unrequired surfaces. They are projections of
the declarations and observations the report reads, not a second specification.
