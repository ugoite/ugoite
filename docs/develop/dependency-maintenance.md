---
title: "Dependency Maintenance"
description: Run the same read-only dependency checks locally that the monthly review runs in CI.
sidebar:
  order: 6
---

The pinned Renovate CLI in GitHub Actions is the regular automated
update-PR writer. `renovate.json` is the policy authority; the Ugoite-owned
GitHub App is scoped to this repository. Renovate runs daily and can also be
dispatched manually. It does not auto-merge. The [Dependency Dashboard](https://github.com/ugoite/ugoite/issues/5099)
lists proposals and holds updates that require human approval or await their
configured schedule.

GitHub Dependency graph and Dependabot alerts are independent of update-PR
generation and remain enabled. Dependabot security-update PR generation is a
separate repository setting and was confirmed disabled at the Renovate
cutover. The monthly `Dependency Review` workflow and the commands below only
report candidates; they never create branches, PRs, or issues, and never edit
dependency files. Promote a candidate only after its minimum quality gate
passes. Held versions, reasons, and the evidence limits for the cutover live in
the [dependency audit](../architecture/security/dependency-audit.md).

The first live Renovate run completed successfully and created the Dashboard,
but no update PR was created because proposals are waiting for human approval
or their configured schedule. The first generated-PR verification remains
open in [#5100](https://github.com/ugoite/ugoite/issues/5100); dry-run success
does not count as generated-PR CI evidence.

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
