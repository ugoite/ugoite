---
title: 'Text-first Composition'
---

**Status: accepted contract.** The current Composition model is defined by
[ADR-027](../../architecture/contracts/adr-027-text-first-composition.md),
[REQ-COMP-010](https://github.com/ugoite/ugoite/blob/main/docs/mitase/requirements/composition/text-first-graph.yaml),
and [REQ-FE-083](https://github.com/ugoite/ugoite/blob/main/docs/mitase/requirements/frontend/composition-text-first.yaml).
This specification does not authorize release preparation, version changes, or
publication.

## Knowledge model

A Composition stores one ordered `spec.variables` graph and a flow layout.
Variables are typed constants, inputs, or query results. A query has one inline
SQL source, explicit SQL-parameter-to-variable bindings, and a server-derived
table or scalar return descriptor. Saved SQL Entry references are not part of
the executable Composition shape.

Display components are exactly Table, Text, and Input:

| Component | Definition |
| --- | --- |
| `table` | One query variable returning an ordered `table<Row>` result |
| `text` | One plain template string with scalar graph references |
| `input` | One input variable with transient user-entered value |

Table output columns, names, aliases, and order come from its SQL result
descriptor. There is no persisted second projection or heading list. Input
values, query results, pages, and editor state remain transient Work.

## SQL and Text templates

The Rust SQL document contract remains the semantic authority for Composition
SQL. Authored `{{name}}` parameters normalize to DataFusion parameters without
altering quoted strings, identifiers, or comments. SQL arguments remain
explicitly bound to compatible scalar variables. Lint, authorized schema
planning, and explicit execution remain separate operations. Composition-only
output wildcard rules do not change ordinary SQL or published Saved SQL.

Text `content` uses `{{variable_id}}` references; `\{{` emits a literal `{{`.
Rust parses ordered literal and reference parts and validates references
against the graph. Missing variables, table values, and JSON values are
rejected. Supported scalar values render as text, without HTML, Markdown, or
expression evaluation. Integer and decimal values stay exact, booleans render
as `true` or `false`, and NULL uses the shared neutral no-value rendering.

## Evaluation and authority

A scalar query returns one column and at runtime exactly one row. Zero rows,
NULL, multiple rows, type drift, and access denial remain distinct outcomes.
Planning one column does not promise one row. Runtime never chooses a first row
or column implicitly.

Planning and evaluation recheck current Form identity, ACL, Publication,
parameter types, SQL validity, and result descriptors. Evaluation uses an
immutable Composition and authority snapshot, publishes only validated
dependency results, and discards stale responses. No result is persisted as
Knowledge or materialized in a hidden table.

## Studio and recovery

Studio opens in Design and has only Design and Variables modes. The Design
palette offers Text, Table, and Input. Text is edited at its placed location;
a compact adjacent control selects Title, Heading, Body, or Caption. Variables
provides the SQL document, graph bindings, and local diagnostics. Creating a
missing variable returns to the pending Design placement only on success.

Reader uses the shared renderer without authoring controls. Save and restore
continue through Space-owned append-only Entry revisions, receipts, and stale
write conflicts. An unsupported historical Composition is not migrated or
executed, while raw inspect, export, history, and restore remain available.
Published Saved SQL API/CLI behavior, the `SQL` system Form, Space/Entry/Form
0.1 semantics, and EntryBrowser/FormTable controls are unchanged.
