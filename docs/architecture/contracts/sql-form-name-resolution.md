---
title: "SQL Form name resolution"
---

This contract covers relation references in normal SQL and Saved SQL. It is
accepted by [ADR-014](decisions.md#adr-014--sql-form-names-resolve-at-execution-and-saved-sql-binds-by-form-id).

## Resolution rules

| Input | Resolution |
| --- | --- |
| `FROM "Expense"` in a normal query | Resolve the exact, case-sensitive name among Forms authorized in the query's Publication. |
| `FROM "Expense"` in Saved SQL | Use the Form ID recorded for `Expense` in the selected Saved SQL revision. |
| `FROM "expense"` | Resolve separately from `"Expense"`; no case folding. |
| `FROM "2025-costs"` | Valid quoted Form name; digits and hyphens are allowed by the Form-name grammar. |
| `FROM form_<UUID>` | Preserve the existing physical relation behavior. |
| Missing, ambiguous, or reserved-name collision | Return an explicit error; never choose one candidate silently. |
| Bound Form deleted, outside the selected Publication, or no longer authorized | Reject execution; never bind to a replacement Form with the same name. |

Only relations in SQL table-reference positions are candidates for Form-name
resolution. The Rust SQL parser handles joins, CTEs, nested queries, quoted
identifiers, and parameters. Literal strings, comments, aliases, and column
references are not rewritten. Name resolution does not grant authorization;
the common query planner still validates every resolved relation.

## Saved SQL revision and integrity

Each new or explicitly updated Saved SQL revision records a binding version and
the exact name-to-Form-ID references derived by the service. The saved SQL body
remains unchanged. A rename does not modify older SQL text or bindings. The
selected Saved SQL revision is read through the existing append-only Entry
history, and its bindings are part of that revision's content-integrity payload.
No read-time migration rewrites old Saved SQL.

When a user edits a renamed reference, the edit is an ordinary Saved SQL update
and produces a new revision. The UI can show the stored reference name beside
the Form's current name as a diagnostic.

## Reader compatibility probe

The previous Saved SQL reader deserializes the complete `metadata` object into
`SqlMetadata`, which is marked `deny_unknown_fields`. A binding-version field
or a bindings field therefore makes that reader return a metadata error. It
does not expose a successful definition with the bindings omitted. An older
Saved SQL revision without these fields continues to decode normally. This
fail-closed behavior is the compatibility boundary; mixed-version writes to a
Space containing bound Saved SQL are unsupported. A rollback must use a reader
that understands the binding metadata.

The regression fixture is a legacy metadata object containing `searchCriteria`
and `generatedName` only, plus a bound metadata object containing
`bindingVersion: 1` and `formBindings`. The first remains readable; the second
must fail in a reader that does not define those fields. Both forms are covered
by the Saved SQL focused tests.

## Focused acceptance fixtures

| Case | Expected result |
| --- | --- |
| Normal name lookup, case distinction, digits/hyphen, and `form_<UUID>` | Both relation forms return the expected rows; `Expense` and `expense` stay distinct. |
| Join, CTE, nested SELECT, quoted name, SQL literal, comment, and parameter | Only table references resolve; all other SQL syntax retains its meaning. |
| Missing name and reserved `form_<UUID>`-shaped Form name collision | Explicit diagnostic, with no candidate selected. |
| Saved SQL create, exact revision read, rename, and explicit edit | Old revision retains the original Form ID; edited SQL creates a new revision. |
| Delete and recreate a Form with the same name | Saved SQL rejects the missing original Form ID and does not use the replacement. |
| Authorization revocation and another Space's Form | Both reads fail even when the saved binding still exists. |
| Page, count, export, cancellation, and continuation | Existing resource bounds and publication pinning remain in force; authorization is rechecked per continuation. |
| Old metadata and unknown binding metadata | Old records remain readable; old readers reject binding metadata without dropping it. |

These fixtures define the focused tests for the implementation PRs. Integration
coverage must exercise the same common Rust resolution path from the CLI and
Browser/API surfaces.
