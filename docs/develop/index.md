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

1. Run Ugoite from source with [Develop Ugoite](../guide/develop/index.md) and
   complete the local authentication flow in
   [Local development login](../guide/develop/local-dev-auth-login.md).
2. Read the [Architecture overview](../architecture/index.md) for boundaries,
   contracts, and the data model.
3. Read the [executable specification](../spec/index.md) for requirements and
   verification evidence.

Engineering Principles, the repository map, and documentation development
guidance land here in the next step. Until then, the guides and Architecture
docs above remain the procedure source.

Validation stays at the repository root with `mise run fmt`, `mise run lint`,
`mise run check`, and `mise run test`. Docsite-focused work uses
`deno task --cwd docsite check` and `deno task --cwd docsite build`.
