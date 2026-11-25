---
title: "Dependency Maintenance"
description: Run the same read-only dependency checks locally that the monthly review runs in CI.
sidebar:
  order: 6
---

The intended automated update-PR writer is the pinned Renovate CLI in GitHub
Actions, using the Ugoite-owned GitHub App scoped to this repository.
`renovate.json` is the policy authority. The workflow runs daily at 01:17 JST
and can also be dispatched manually. Single-writer completion is still being
verified in [#5120](https://github.com/ugoite/ugoite/issues/5120). Eligible
routine updates are grouped by ecosystem and
open PRs immediately, without Dashboard approval or manager-specific
schedules. There is no configured concurrent-PR or hourly cap. Automatic
merging remains disabled. The [Dependency Dashboard](https://github.com/ugoite/ugoite/issues/5099)
lists detected dependencies, grouped proposals, and held updates.

GitHub Dependency graph and Dependabot alerts are managed independently from
update-PR generation. The Renovate workflow verifies that its App can access
the alerts API; it does not read or change the separate security-update PR
setting. The monthly `Dependency Review` workflow and the commands below only
report candidates; they never create branches, PRs, or issues, and never edit
dependency files. Promote a candidate only after its minimum quality gate
passes. Held versions, reasons, and the evidence limits for the cutover live in
the [dependency audit](../architecture/security/dependency-audit.md).

The initial live Renovate run before ecosystem grouping created the Dashboard
but no update PR because its then-current approval and schedule gates held
proposals. Those gates have since been removed, and the grouped writer run
created candidates #5115–#5119. Current candidate heads, required CI results,
and the remaining cutover evidence are recorded in the
[dependency audit](../architecture/security/dependency-audit.md),
[#5100](https://github.com/ugoite/ugoite/issues/5100), and
[#5158](https://github.com/ugoite/ugoite/issues/5158). A dry-run or successful
extraction does not replace CI evidence on a generated candidate.

```bash
mise outdated --bump
deno outdated --recursive
cargo audit --json
cargo metadata --locked --no-deps --format-version 1
deno audit
```

`cargo-audit 0.23.0` reads the workspace `Cargo.lock` but does not accept a
`--locked` flag. Use `cargo metadata --locked` separately when checking that
the workspace resolves without changing its lockfile.

`cargo audit` and `deno audit` run only when the corresponding tool is
available; the workflow does not install extra scanners on its own. If the
scheduled review ever stops providing value, delete the workflow: development
and distribution do not depend on it.
