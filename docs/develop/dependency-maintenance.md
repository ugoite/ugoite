---
title: "Dependency Maintenance"
description: Run the same read-only dependency checks locally that the monthly review runs in CI.
sidebar:
  order: 6
---

Dependabot is the only automated update-PR generator. The monthly
`Dependency Review` workflow and the commands below only report candidates;
they never create branches, PRs, or issues, and never edit dependency files.
Promote a candidate only through a normal change PR after its minimum gate
passes. The single record for held pins and accepted exceptions is the
[dependency audit](../architecture/security/dependency-audit.md).

```bash
mise outdated --bump
deno outdated --recursive
cargo audit --locked --json
deno audit
```

`cargo audit` and `deno audit` run only when the corresponding tool is
available; the workflow does not install extra scanners on its own. If the
scheduled review ever stops providing value, delete the workflow: development
and distribution do not depend on it.
