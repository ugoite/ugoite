---
title: "Text-first Composition"
---

**Status: accepted contract.** [ADR-027](../../architecture/contracts/adr-027-text-first-composition.md)
records the design decision. Behavior is specified by
[REQ-COMP-010](https://github.com/ugoite/ugoite/blob/main/docs/mitase/requirements/composition/text-first-graph.yaml);
browser behavior is specified by
[REQ-FE-083](https://github.com/ugoite/ugoite/blob/main/docs/mitase/requirements/frontend/composition-text-first.yaml).
This page summarizes implementation architecture; it does not authorize
release preparation, version changes, or publication.

## Knowledge model

The executable document stores one ordered `spec.variables` graph and a flow
layout. Per REQ-COMP-010, variables are typed constants, inputs, or inline-SQL
queries with explicit parameter bindings and a server-derived table or scalar
descriptor. Components are Table, Text, and Input; Saved SQL Entry references
are not executable Composition sources. Table columns follow SQL projection
order without a second saved projection. Input values, query results, pages,
and editor state are transient Work.

## SQL and Text templates

The Rust SQL document contract owns Composition SQL meaning. Authored
`{{name}}` parameters normalize to DataFusion parameters without altering
quoted text, identifiers, or comments; arguments bind explicitly to compatible
scalar variables. Lint, authorized schema Check, and execution remain separate.
Check may select one query root and includes its transitive dependencies;
Save validates the full graph. Composition's output-wildcard lint does not
change ordinary SQL or published Saved SQL.

Rust validates Text's ordered literal/reference parts; `\{{` is a literal
`{{`. Missing references, table values, and JSON values are rejected. Scalar
values render as text (not HTML, Markdown, or expressions); integers and
decimals stay exact, booleans use `true` or `false`, and NULL uses the shared
neutral no-value rendering.

## Evaluation and authority

REQ-COMP-010 defines scalar cardinality and failure semantics. Planning and
evaluation recheck current Form identity, ACL, Publication, parameter types,
SQL validity, and result descriptors. Evaluation uses an immutable Composition
and authority snapshot, publishes only validated dependency results, and
discards stale responses. Results are transient; they are neither persisted as
Knowledge nor materialized in a hidden table.

## Studio and recovery

REQ-FE-083 owns Studio and Reader behavior: Studio has Design and Variables
modes with Text, Table, and Input; Reader has no authoring controls. Space-owned
save and restore use append-only Entry revisions, receipts, and stale-write
conflicts. Unsupported historical Compositions are not migrated or executed;
raw inspect, export, and history remain available, and restore supports only
the current shape. Published Saved SQL API/CLI behavior, the `SQL` system Form,
Space/Entry/Form 0.1 semantics, and EntryBrowser/FormTable controls are
unchanged.
