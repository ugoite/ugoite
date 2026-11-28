---
title: "Dependency Maintenance"
description: Run read-only dependency checks and review proposed updates safely.
sidebar:
  order: 6
---

`renovate.json` owns automated update policy. The Renovate workflow proposes
updates as pull requests; it does not merge them automatically. Current
candidate status belongs to the pull requests and the
[Dependency Dashboard](https://github.com/ugoite/ugoite/issues/5099), not to a
copied status table in documentation.

GitHub Dependency graph and Dependabot alerts are managed separately from
routine update pull requests. The Renovate workflow's alerts API preflight
checks access; it does not replace an authorized alert inventory. The open
[alert-triage issue](https://github.com/ugoite/ugoite/issues/4997) tracks that
work. Active compatibility holds and their reasons are listed in the
[dependency audit exceptions](../architecture/security/dependency-audit.md).

## Read-only checks

These commands report candidates or findings. They do not edit manifests,
lockfiles, create branches, or open pull requests.

```bash
mise outdated --bump
deno outdated --recursive
cargo audit --json
cargo metadata --locked --format-version 1
deno audit
```

`cargo-audit` does not accept a `--locked` flag. Use `cargo metadata --locked`
separately to verify that the workspace resolves without changing `Cargo.lock`.
Omit `--no-deps`, which skips dependency resolution. `cargo
audit` and
`deno audit` run only when their tools are available; the monthly Dependency
Review workflow does not install additional scanners.

## Reviewing an update

Review the exact candidate pull request and its affected manifest/lockfile
changes. A diagnostic or successful update-writer run is not candidate test
evidence. Require the applicable CI and CodeQL checks on the candidate head,
then use the normal repository protections and merge queue. Do not weaken a test
or add a compatibility hold to hide an unexplained failure; investigate and
record durable unresolved work in its existing issue.

Dependency Review is read-only and independent of development and release. If
its reports stop providing value, remove that workflow rather than giving it
write access.
