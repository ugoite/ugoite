---
title: "Develop Ugoite"
description: Run the repository from source and complete the local authentication path.
sidebar:
  label: "Overview"
  order: 1
---

Development starts at the repository root and uses the same Rust server,
frontend, docsite, and validation tasks as CI. The browser remains server-backed
in the current release.

## Seed local sample data

Use the root task to create a portable sample Space for local development:

```bash
mise run seed
```

The default root is `./data`; the generated Space directory uses its immutable
UUIDv7 and the `dev-seed` slug. It uses the `renewable-ops` scenario and
approximately 50 entries. Pass arguments after `--` or set the `UGOITE_SEED_*`
environment variables documented by `scripts/dev-seed.sh` when a different root,
Space ID, scenario, entry count, or deterministic seed is needed:

```bash
mise run seed -- --space-id demo --scenario lab-qa --entry-count 10 --seed 7
```

The helper refuses to overwrite an existing target Space. Choose another Space
ID or remove the local development data intentionally before seeding again.

## Development path

1. Follow [Local development login](local-dev-auth-login.md) to start from
   source and complete the first-run authentication flow.
2. Use the repository root `mise` tasks for formatting, checks, tests, and the
   docsite build.
3. When changing the browser or API boundary, read the matching
   [architecture](../../architecture/index.md) and
   [executable specification](../../spec/index.md).
