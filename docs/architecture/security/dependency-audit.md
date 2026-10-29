---
title: Dependency audit exceptions
---

This page records where each dependency and toolchain version is decided,
which areas Dependabot monitors, which pins are held manually, and which
security exceptions are accepted. Version meaning lives in the manifests
listed below; this page is the record, not a second authority.

## Authority: version meaning lives in manifests

| Domain | Authority file | Notes |
| --- | --- | --- |
| Rust crates | `Cargo.toml` / `Cargo.lock` | `cargo ... --locked` is the enforced mode in CI and release builds. |
| Deno workspace | `deno.json` (root + `frontend` / `docsite` / `e2e` / `tools`) + shared `deno.lock` | Frozen install is the enforced mode once the B3 lockfile gate lands; until then `deno.lock` is committed and CI restores it via cache. |
| Rust compiler | `rust-toolchain.toml` (planned in B2; currently `mise.toml` `rust = "1.95.0"`) | Single compiler authority for dev, CI, WASM, and Docker source build. |
| Deno runtime (dev/CI) | `mise.toml` (`tools.deno`) | Docker `denoland/deno:<version>` stays an explicit fixed tag and must match this authority while the same runtime is intended (parity gate planned in B3). |
| Node (CI/release) | `.github/workflows/ci.yml`, `release-candidate.yml`, `release-publish.yml` | CI runs Node 24; release packaging still runs Node 20 until the B3 Node-24 proof lands. `packages/ugoite` `engines.node >= 20` is the distribution support floor, not the CI host version. |

`docs/architecture/security/dependency-audit.md` is the single record for
exceptions. Scan reports are evidence, not a second authority.

## Monitored by Dependabot (update PR generator)

Dependabot is the only automated update-PR generator. No Renovate, no custom
updater, no workflow that pushes dependency changes.

| Ecosystem | Scope | Cadence |
| --- | --- | --- |
| `github-actions` | `/` | daily (`all-dependencies` group) |
| `cargo` | `/` | daily (`all-dependencies` group) |
| `docker` | `/` | daily (`all-dependencies` group) |
| `npm` | `/packages/ugoite` | daily (`all-dependencies` group) |
| `devcontainers` | `/` | daily (`all-dependencies` group) |
| `deno` | `/`, `/frontend`, `/docsite`, `/e2e`, `/tools` | weekly, independent frame (planned in B1) |
| `rust-toolchain` | `/` | monthly (planned in B2 with the `rust-toolchain.toml` migration) |

## Intentionally held outside Dependabot automation

| Target | Pinned state at B0 | Fixed reason | Update trigger | Required verification | Revisit |
| --- | --- | --- | --- | --- | --- |
| `arrow*` / `parquet` / `iceberg*` / `datafusion*` / `opendal*` major+minor | Dependabot `ignore` for `version-update:semver-major,minor`; patch stays grouped | Arrow concrete types are shared across the stack; single-crate bumps break compatibility | Upstream publishes a compatible stack | `iceberg` 2-crate same SHA, Arrow/Parquet/DataFusion/OpenDAL/Space compat, SQL + storage regression | quarterly |
| `iceberg` + `iceberg-datafusion` Git revision | `rev=1ecd497` in `crates/ugoite-iceberg/Cargo.toml` | Git-revision-only upstream area; SHA rewrite is never automated | Upstream compatible revision | Saved-Space reopen, old-reader compat, bounded query, storage/API regression | monthly |
| Mitase `0.2.5` + 4-target SHA256 (`tools/mitase.lock.toml`) | version + checksums pinned | Bootstrap and Mitase contract depend on exact bytes | New upstream release | version + all 4 checksums together, bootstrap + Mitase contract tests | monthly |
| MinIO digest (`MINIO_IMAGE` in `ci.yml`) | `pgsty/minio@sha256:2b361...` | S3/ACL scenario depends on exact image | New upstream digest/release | S3/ACL scenario, cache + CI health | monthly |
| sccache executed version (`v0.17.0` in `ci.yml`) | pinned action version | Cache + CI behavior depends on exact runner | New upstream release | Cache behavior + CI health | monthly |
| `mise` Deno / Helm / nextest / mise minimum version | `mise.toml`: Deno `2.8.3`, Helm `3.19.0`, nextest `cargo-nextest-0.9.143`, `min_version 2025.11.3` | Dev/CI tool behavior baseline; mise itself has no Dependabot ecosystem | `mise outdated` shows a candidate | Affected `mise` task, dev/CI behavior | monthly |
| Deno inline version specifiers | `npm:astro@...`, `npm:@playwright/test@...`, `npm:sharp@...` in tasks/sources | Dependabot does not track versions embedded in command strings or TS sources | `deno outdated --recursive` + known-duplicate check | `frontend`/`docsite`/E2E alias-vs-entity agreement | monthly |
| Dev Container / OS packages | `devcontainer.json` base image + feature pins | Base image rebuild is periodic, not per-PR | Base image update + rebuild | devcontainer setup, shell quality, basic E2E | periodically |
| RustSec advisories | `rsa 0.9.10` (`RUSTSEC-2023-0071`) unresolved; `event-listener` pinned at `5.4.2` | No patched upstream release at this boundary; see below | Patched `rsa` release or supported replacement | `cargo audit --locked --json` review | each graph/channel change |

## Current RustSec exception detail

The current locked dependency graph has one unresolved RustSec advisory for
`rsa 0.9.10` (RUSTSEC-2023-0071). No patched upstream release is available for
the advisory at this version boundary.

The dependency is transitive through two separately scoped surfaces:

- `openidconnect`, used by the server's supported OIDC protocol adapter;
- OpenDAL/reqsign cloud backends, used by S3/GCS/Azure/OSS storage support.

The package is not used by the local-first core data path. Removing it would
require dropping those optional transport/authentication surfaces, so this
release keeps the locked graph and records the exception rather than silently
ignoring the audit result. `event-listener` is pinned through the lockfile at
5.4.2, which removes its fixable advisory.

Operators should rerun `cargo audit --locked --json` when the dependency graph
or release channel changes. The exception must be removed as soon as an
upstream patched `rsa` release or a supported replacement is available.
Do not record this exception as resolved without re-running the audit.

## Monthly read-only check (no writes)

The planned `dependency-review` workflow (B4) and the local commands below only
report. They never create branches, PRs, issues, push, or edit dependency
files.

```bash
mise outdated
deno outdated --recursive
cargo audit --locked --json
deno audit
```

Results go to the Actions Job Summary. Promote only what passes its
minimum gate in the table above through a normal change PR.
