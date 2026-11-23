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
| Update policy | `renovate.json` | Defines enabled managers, grouped updates, schedules, approval gates, compatibility holds, and the no-auto-merge policy. This is the repository's update-policy authority. |
| Update-PR runner | `.github/workflows/renovate.yml` | Runs the pinned Renovate CLI on a daily schedule and manual dispatch using a GitHub App scoped to this repository. It is the regular update-PR writer. |
| Vulnerability alerts | GitHub Dependency graph and Dependabot alerts settings | Remain enabled independently of update-PR generation. Dependabot security-update PR generation is separately managed in repository settings and was confirmed disabled at cutover. |
| CI lane selection | `tools/ci-impact.ts` and the required CI workflow | Dependency manifests and lockfiles select the existing affected quality lanes. Unknown changes fall back to the full lane set. A Deno lockfile change selects all lanes. |
| Accepted exceptions | This page | Record the reason, update trigger, and required verification here; do not use this page as a second version source. |

The monthly `Dependency Review` workflow and the matching local commands only
report candidates. They do not edit dependency files, create branches, or
open pull requests. CI caches and runner environments are rebuildable work;
they are not dependency authorities.

Renovate groups routine GitHub Actions, Cargo, Docker, the npm package under
`/packages/ugoite`, and Dev Container updates in `all-dependencies`. The five
Deno manifests share the weekly `deno-workspace` group and root `deno.lock`.
The Cargo Arrow stack uses a separate review group for major/minor changes;
routine patches remain eligible. Rust toolchain updates remain monthly. The
exact rules and approval gates live in `renovate.json`.

## Deno grouped update status

The five Deno manifests share the root `deno.lock`; Renovate proposes routine
updates through the weekly `deno-workspace` group. The live writer run created
the Dependency Dashboard but did not create an update PR: the Deno proposal is
pending human approval there. The old Dependabot Deno PR #5082 was closed after
the writer switch. Until a human approves a proposal and its generated PR
passes frozen install and required CI, the actual Deno PR path remains
unverified. Track that check in [#5100](https://github.com/ugoite/ugoite/issues/5100).

The App-backed inventory covers all five Deno manifests. The successful
extract, lookup, and full dry-runs do not prove a generated Deno PR passes CI.
See the run record below for the exact sources and limits of each result.

## Intentionally held updates and security exceptions

| Target | Current held state | Reason | Revisit trigger and verification |
| --- | --- | --- | --- |
| Arrow / Parquet / Iceberg / DataFusion / OpenDAL stack | Renovate groups Cargo major/minor changes into `rust-arrow-stack` and requires Dependency Dashboard approval; routine patches remain eligible. | The stack shares concrete Arrow types; a single-crate major or minor update can break compatibility across the stack. | Approve a coordinated candidate only after review; validate the crates, Iceberg adapters, SQL, and storage regressions together. Review quarterly. |
| `iceberg` and `iceberg-datafusion` Git dependencies | Revision `1ecd497` in `crates/ugoite-iceberg/Cargo.toml`. | This upstream area is consumed by Git revision, not a normal registry version update. | Review a compatible upstream revision and verify saved-Space reopen, bounded queries, storage, and API behavior monthly. |
| Mitase bootstrap | Version `0.2.5` and four target SHA-256 values in `tools/mitase.lock.toml`. | Bootstrap and Mitase contracts depend on the exact downloaded bytes. | Update version and all four checksums together; run bootstrap and Mitase contract checks monthly. |
| MinIO CI image | `pgsty/minio@sha256:2b36182f3479c58b5cba920f20479738ee85ce218de0596a244f9a1368268db9` in `.github/workflows/ci.yml`. | The S3 authorization scenario uses a specific image build. | For a new image digest, rerun the S3/ACL scenario and the affected CI lane. Review monthly. |
| sccache action | `mozilla-actions/sccache-action` in `.github/workflows/ci.yml` is pinned to commit `fc920bf0ec8de6ee65d409111f7ec508035751ba` (upstream tag comment `v0.0.11`). The hosted runner reported sccache `0.17.0`. | The action and cache behavior affect CI execution. The action revision, rather than an independently installed binary version, is pinned. | On an action or runner change, verify cache setup and affected CI lanes. Review monthly. |
| TypeScript major updates | TypeScript 7 remains visible; Renovate's global major-update policy requires Dependency Dashboard approval. Lookup proposed 7.0.2. | TypeScript 7 removes compiler APIs used by `tools/check_frontend_changed_types.ts`. | Review the candidate after that consumer supports the new APIs; validate frontend changed-file type checking. |
| Starlight | Renovate holds `0.42.1`–`0.42.11` and `0.43.0`–`0.43.1` in `renovate.json`; `0.42.0` remains eligible. | Each observed held grouped candidate failed static rendering because Deno could not resolve `astro:middleware`. | Revisit a later candidate; verify Docsite build and navigation tests. |
| Playwright | Renovate holds `1.61.0`, `1.61.1`, `1.62.0`, `1.62.1`, `1.63.0`, `1.64.0`, `1.64.1`, `1.65.0`, and `1.65.1`. | The held targets resolved to `1.65.1` and Docsite navigation tests loaded as CommonJS, where `import.meta` failed. | Revisit with a candidate that preserves module loading; verify Docsite navigation and E2E browser setup. |
| SolidStart | Renovate holds `2.0.0`–`2.0.6`, `2.1.0`, and `2.2.0`–`2.2.3`; `1.5.1` remains eligible. | The held targets resolved to `2.2.3`, which removes `defineConfig` used by `frontend/app.config.ts`. | Revisit a compatible candidate; verify frontend type checking, tests, and build. |
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

## Renovate cutover evidence

The writer cutover merged through the merge queue in PR
[#5094](https://github.com/ugoite/ugoite/pull/5094) at source SHA
`e7a3006bed006b5ad1976431930a725ef46ea0c4`. The repository administrator
confirmed that Dependabot security-update PR generation is disabled while the
Dependency graph and Dependabot alerts remain enabled. The current integration
cannot independently read that repository setting.

| Evidence | Source | Result and limit |
| --- | --- | --- |
| App-backed extract | [run 45161867473](https://github.com/ugoite/ugoite/actions/runs/45161867473), main `db7559e4f818aec11a513915ea9011d08b1eaba9` | Success; inventory covered 36 files, 643 dependencies, and all five Deno manifests. |
| App-backed lookup | [run 45164810651](https://github.com/ugoite/ugoite/actions/runs/45164810651), same main SHA | Success; candidate lookup is not CI evidence. Pre-job run 45161895265 was cancelled and replaced. |
| App-backed full dry-run | [run 45161915530](https://github.com/ugoite/ugoite/actions/runs/45161915530), same main SHA | Success; no repository writes. |
| First live writer run | [run 45191937781](https://github.com/ugoite/ugoite/actions/runs/45191937781), cutover SHA `e7a3006bed006b5ad1976431930a725ef46ea0c4` | Success; App alert access succeeded and Renovate created [Dependency Dashboard #5099](https://github.com/ugoite/ugoite/issues/5099). No update PR was created because proposals await human approval or their configured schedule. Generated-PR CI remains unobserved; follow-up [#5100](https://github.com/ugoite/ugoite/issues/5100) tracks it. |

The lookup and live run reported that Renovate could not determine a digest
for the manually managed `iceberg-rust` Git tag because the upstream tag has
a `v` prefix. The live run also logged Cargo/Docker grouped-update collisions.
Neither warning failed the run. The Iceberg revision remains manually managed;
do not mark these diagnostics as a passed dependency update.

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
