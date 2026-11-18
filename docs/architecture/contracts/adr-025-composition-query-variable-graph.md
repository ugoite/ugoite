---
title: "ADR-025 — Composition query variable graph and complete SQL Workspace"
---

**Status: Graph decision retained. Authoring and renderer decisions superseded
before release by [ADR-027](adr-027-text-first-composition.md).**

One typed dependency graph unified inputs, calculations, and presented values
with explicit parameter bindings. Stateless, authorized evaluation reads one
snapshot. The owning Entry revision and its append-only history keep
Composition portable. [The current contract](composition.md) describes these
invariants and the Text-first model that replaced the earlier query variants,
renderers, and SQL Workspace requirements.

[Original decision](https://github.com/ugoite/ugoite/blob/6bcb5426b937f7981fca1fb97ae70c035651da89/docs/architecture/contracts/adr-025-composition-query-variable-graph.md).
