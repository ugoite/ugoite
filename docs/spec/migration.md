---
title: "Migration status"
description: Which specification domains are canonical in Mitase and which remain in docs/spec.
---

This page records the migration ledger. It preserves source meaning: missing or
ambiguous evidence becomes an explicit gap and never narrows a Requirement.
Mitase validates declared relationships and evidence; it does not execute tests
or become a second Knowledge authority.

## Migrated domain authority

Foundation, Policy, Search, Entry, Form, Indexer, API, Asset, Frontend, E2E,
Integrity, and the Storage Space foundation, authenticated creation contract,
plus connector/access/routing/preference slice are represented in the canonical
Mitase records at `docs/mitase` for the current dogfood slice. These records are
the semantic source of truth for the migrated domains; their corresponding
legacy Foundation, Policy, Requirement, and Feature YAML are migration evidence
only and cannot override the canonical representation. The API-specific legacy
requirement registry is retired and is no longer part of Mitase's declared
inventory. The canonical API graph is the only semantic authority for that
domain. Frontend legacy requirement YAML is likewise retired and no longer part
of Mitase's declared inventory. The Asset and E2E requirement YAML have been
retired entirely. The legacy Indexer requirement registry is likewise retired;
its canonical Search and Form graphs are the only semantic authority for derived
indexing, structured query, word-count, and validation behavior. The legacy
Search requirement registry is also retired; the canonical Search graph at
`docs/mitase/requirements/search.yaml` is the only semantic authority for
keyword search, structured query, frontend search behavior, and derived relation
maintenance. The legacy Entry requirement registry is also retired; the
canonical Entry graph at `docs/mitase/requirements/entries.yaml` is the only
semantic authority for Entry creation, revision, mutation, history, Markdown
extraction, and interface behavior. The legacy Form requirement registry is also
retired; the canonical Form graph at `docs/mitase/requirements/forms.yaml` is
the only semantic authority for Form schema governance, CRUD operations,
reserved metadata, row references, attribution, and typed property conversion.
The legacy Frontend requirement registry is also retired; the canonical Frontend
graph at `docs/mitase/requirements/frontend.yaml` is the only semantic authority
for routes, components, interaction surfaces, API clients, and exact Frontend
verification evidence. The legacy Integrity requirement registry is likewise
retired and no longer part of Mitase's declared inventory. The migrated Storage
Space foundation records are likewise no longer semantic authority in their
legacy registry. The duplicate-create conflict contract is now canonical;
remaining Storage records continue to be migrated in focused slices.
Changed-ownership enforcement remains staged until it can be scoped safely to
the migrated slice. The canonical Operations graph now represents `REQ-OPS-001`
through `REQ-OPS-024`, together with `REQ-OPS-043` and `REQ-OPS-044`; later
Operations records remain migration evidence until their focused Mitase slices
are reviewed. Other requirement and feature domains remain authoritative in
their existing `docs/spec` records until migrated. The retired Asset requirement
registry is not retained as a second semantic source; its canonical replacement
is the Mitase Asset graph described above.

`docs/mitase` is an intentional Ugoite specification surface for the Mitase
schema, not a second product authority. As legacy registry machinery and
unmigrated domains are retired, their corresponding `docs/spec` records may be
removed after the equivalent canonical records, evidence, and scoped ownership
rules have been reviewed.
