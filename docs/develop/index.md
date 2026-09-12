---
title: "Develop Ugoite"
description: Build, test, and extend Ugoite from source.
sidebar:
  label: "Overview"
  order: 1
---

Develop Ugoite is for contributors. It explains the repository map, the thin
adapter architecture, and the validation workflow in the order a new contributor
needs them.

## Development path

1. Read [Engineering Principles](engineering-principles.md) for why the
   architecture stays stable while product discovery continues.
2. Read [Repository Map](repository-map.md) to find where behavior lives.
3. Run Ugoite from source with [Develop Ugoite](../guide/develop/index.md) and
   complete the local authentication flow in
   [Local development login](../guide/develop/local-dev-auth-login.md).
4. Read [Documentation Development](documentation.md) before adding or moving
   prose.

## Architecture re-entry points

- Start with the [Architecture overview](../architecture/index.md), then
  [System boundaries](../architecture/boundaries/index.md) and
  [Security architecture](../architecture/security/index.md).
- Check normative behavior in
  [Architecture contracts](../architecture/contracts/overview.md), the
  [Space compatibility contract](../architecture/contracts/space-compatibility.md),
  and the [Data model overview](../architecture/data-model/overview.md).
- Verify behavior through the [executable specification](../spec/index.md),
  whose registries point back to implementation and tests.

Validation stays at the repository root with `mise run fmt`, `mise run lint`,
`mise run check`, and `mise run test`. Docsite-focused work uses
`deno task --cwd docsite check` and `deno task --cwd docsite build`.
