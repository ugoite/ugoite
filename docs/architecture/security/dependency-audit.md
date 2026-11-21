---
title: Dependency audit exceptions
---

This page records update exceptions, their reasons, and the evidence needed to
revisit them. It is not a version authority: manifests, lockfiles, toolchain
files, and workflow configuration below decide the actual versions and update
policy.

## Authorities and verification

| Domain | Authority | Verification and notes |
| --- | --- | --- |
| Rust crates | Workspace `Cargo.toml` files and `Cargo.lock` | CI and release builds use locked Cargo operations. |
| Deno dependencies | Root and member `deno.json` files plus the single root `deno.lock` | `mise run deps:deno:verify` runs `deno install --frozen --allow-scripts=npm:@tailwindcss/oxide,npm:esbuild,npm:sharp`. `setup`, the hosted Web lane, and artifact preparation call this task. The lockfile-hash cache is disposable; the frozen install verifies the manifest-to-lockfile boundary. Deno command and source imports use manifest aliases rather than embedding a second package version. |
| Rust compiler | `rust-toolchain.toml` | The same channel and components are used by local development and CI. `mise.toml` enables mise's idiomatic Rust version file instead of duplicating a compiler pin. |
| Development tools | `mise.toml` | This file pins Deno `2.9.8`, Helm `3.19.0`, nextest `cargo-nextest-0.9.143`, and the minimum mise version `2025.11.3`. Rust follows `rust-toolchain.toml`. |
| Docker Deno runtime | `Dockerfile` | The frontend build image is explicitly pinned to `denoland/deno:2.9.8`, matching the development toolchain. |
| Node runtime | `.github/workflows/ci.yml`, `release-candidate.yml`, and `release-publish.yml` | CI and release workflows use Node 24. `packages/ugoite/package.json` declares `engines.node >=20` as the package support floor. |
| Update automation | `.github/dependabot.yml` | Dependabot is the automated update-PR generator. Deno monitors all five workspace directories weekly in one wildcard group with a regular update-PR limit of one. Security updates use GitHub's separate security-update handling. |
| CI lane selection | `tools/ci-impact.ts` and the required CI workflow | Dependency manifests and lockfiles select the existing affected quality lanes. Unknown changes fall back to the full lane set. A Deno lockfile change selects all lanes. |
| Accepted exceptions | This page | Record the reason, update trigger, and required verification here; do not use this page as a second version source. |

The monthly `Dependency Review` workflow and the matching local commands only
report candidates. They do not edit dependency files, create branches, or
open pull requests. CI caches and runner environments are rebuildable work;
they are not dependency authorities.

GitHub Actions, Cargo, Docker, the npm package under `/packages/ugoite`, and
Dev Containers remain in the daily `all-dependencies` multi-ecosystem group.
Rust toolchain updates remain monthly.

## Deno update grouping evidence

The five Deno manifests share the root `deno.lock`. Dependabot now keeps those
directories in one weekly wildcard version-update group and limits regular
Deno update PRs to one open PR. Exact compatibility holds are listed below;
security updates remain independently visible.

The first generated grouped update, [PR #4984](https://github.com/ugoite/ugoite/pull/4984),
included the five manifests and shared lockfile but failed the Docsite build
with Starlight `0.43.1` and Docsite navigation tests with Playwright `1.65.1`.
A later grouped update, [PR #4989](https://github.com/ugoite/ugoite/pull/4989),
also exposed failures with Starlight `0.43.0`, Playwright `1.65.0` (resolved
to `1.65.1` in the lockfile), and SolidStart `2.2.3`. The observed failures
and the exact holds are recorded below; they do not justify suppressing other
versions or all major updates.

[Issue #4693](https://github.com/ugoite/ugoite/issues/4693) tracks verification
of the next Dependabot-generated grouped update. The grouping policy is in
place, but workspace coverage, a consistent shared lockfile, frozen
verification, and required CI still need to be evidenced on an actual update
PR before this rollout is considered complete. Existing update PRs should be
closed only after their candidates are accounted for in the grouped PR.

## Intentionally held updates and security exceptions

| Target | Current held state | Reason | Revisit trigger and verification |
| --- | --- | --- | --- |
| Arrow / Parquet / Iceberg / DataFusion / OpenDAL stack | Dependabot suppresses major and minor updates; patch updates remain eligible and grouped. | The stack shares concrete Arrow types; a single-crate major or minor update can break compatibility across the stack. | Propose a compatible stack update; validate the coordinated crates, Iceberg adapters, SQL, and storage regressions together. Review quarterly. |
| `iceberg` and `iceberg-datafusion` Git dependencies | Revision `1ecd497` in `crates/ugoite-iceberg/Cargo.toml`. | This upstream area is consumed by Git revision, not a normal registry version update. | Review a compatible upstream revision and verify saved-Space reopen, bounded queries, storage, and API behavior monthly. |
| Mitase bootstrap | Version `0.2.5` and four target SHA-256 values in `tools/mitase.lock.toml`. | Bootstrap and Mitase contracts depend on the exact downloaded bytes. | Update version and all four checksums together; run bootstrap and Mitase contract checks monthly. |
| MinIO CI image | `pgsty/minio@sha256:2b36182f3479c58b5cba920f20479738ee85ce218de0596a244f9a1368268db9` in `ci.yml`. | The S3 authorization scenario uses a specific image build. | For a new image digest, rerun the S3/ACL scenario and the affected CI lane. Review monthly. |
| sccache action | `mozilla-actions/sccache-action` is pinned to commit `fc920bf0ec8de6ee65d409111f7ec508035751ba` (upstream tag comment `v0.0.11`). The hosted runner reported sccache `0.17.0`. | The action and cache behavior affect CI execution. The action revision, rather than an independently installed binary version, is pinned. | On an action or runner change, verify cache setup and affected CI lanes. Review monthly. |
| TypeScript major updates | Dependabot ignores semver-major updates for `typescript`; patch and minor updates remain eligible. | TypeScript 7 removes compiler APIs used by `tools/check_frontend_changed_types.ts`. | Remove the ignore after that consumer supports the new APIs; validate frontend changed-file type checking. |
| Starlight | Dependabot holds only `0.43.0` and `0.43.1`. | Both observed candidates failed static rendering because Deno could not resolve `astro:middleware`. | Revisit when a candidate is available; verify Docsite build and navigation tests. |
| Playwright | Dependabot holds update target `1.65.0` and resolved version `1.65.1`. | The update caused Docsite navigation tests to load as CommonJS, where `import.meta` failed. | Revisit with a candidate that preserves module loading; verify Docsite navigation and E2E browser setup. |
| SolidStart | Dependabot holds only `2.2.3`. | That release removes `defineConfig`, which `frontend/app.config.ts` uses. | Revisit when a compatible candidate is available; verify frontend type checking, tests, and build. |
| RustSec `rsa` advisory | The lockfile contains `rsa 0.9.10`, reported for `RUSTSEC-2023-0071`. | The crate is transitive through the OIDC adapter and OpenDAL cloud backends. Removing those optional protocol and storage surfaces is not an appropriate dependency-update workaround. | Rerun `cargo audit --locked --json` on graph changes and remove this exception when a patched release or supported replacement is available. |
| Dev Container and OS packages | Base image and feature pins are in `.devcontainer/devcontainer.json`. | Base image and OS package updates are periodic environment changes rather than application dependency updates. | Review with a base-image update; verify container setup and the basic contributor workflow. |

The current frontend manifest resolves the Vitest runner and coverage provider
to `5.0.5`, while its `vitest/config` and subpath aliases resolve to `4.1.8`.
Docsite resolves Vitest to `4.1.8` and `@vitest/coverage-v8` to `5.0.5`.
These version differences are not, by themselves, evidence of
incompatibility: the hosted frontend coverage lane and the Docsite coverage
task passed with the current aliases. When changing either package, run the
corresponding coverage task and its normal test/build lane.

## RustSec exception detail

The current lockfile contains `rsa 0.9.10`, reported under
`RUSTSEC-2023-0071`. The dependency is transitive through `openidconnect` and
OpenDAL/reqsign cloud backends used for S3, GCS, Azure, and OSS storage. It is
not used by the local-first core path. Keep the finding visible in the
read-only audit and remove this exception when a patched release or supported
replacement is available; do not mark it resolved without rerunning the audit.

The current lockfile resolves `event-listener` to `5.4.2`; it is not a
separately maintained exception. Re-run the audit when the Rust dependency
graph changes.

## Monthly read-only check

The scheduled workflow and local commands below report only. Results appear
in the Actions Job Summary; promote candidates through a normal change PR only
after their affected verification passes.

```bash
mise outdated --bump
deno outdated --recursive
cargo audit --locked --json
deno audit
```
