# IEapp Spec (v2)

This directory contains the **current, canonical** specification for IEapp.

Start here:
- [index.md](index.md) - Master navigation

## What lives here

- **Architecture**: [architecture/](architecture/)
- **API**: [api/](api/)
- **Data Model**: [data-model/](data-model/)
- **Stories** (YAML): [stories/](stories/)
- **Requirements** (YAML + test mapping): [requirements/](requirements/)
- **Feature Registry** (API-level): [features/](features/)
- **Security & Testing**: [security/](security/), [testing/](testing/)

## Conventions

- **"Class" is the user-facing term** (legacy term: schema).
- **API paths**: Backend paths are canonical and include the `/api`
  prefix; frontend calls the same path *without* `/api` (the client
  adds the prefix).
- **Machine-readable docs**: Stories, Requirements, and Features are expressed
  in YAML so we can validate doc↔code consistency in tests.

## Editing rules

- When adding a new requirement, update the appropriate file in
  [requirements/](requirements/) and ensure the verifying tests exist.
- When adding/removing endpoints, update the per-kind YAML files under
  [features/](features/) so code navigation stays accurate.
