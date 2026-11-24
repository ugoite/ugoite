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
| Update policy | `renovate.json` | Defines enabled managers, one group per ecosystem, compatibility holds, immediate PR creation, and the no-auto-merge policy. This is the repository's update-policy authority. |
| Update-PR runner | `.github/workflows/renovate.yml` | Runs the pinned Renovate CLI on a daily schedule and manual dispatch using a GitHub App scoped to this repository. Live evidence below records update candidates and writer identity. |
| Vulnerability alerts | GitHub Dependency graph and Dependabot alerts settings | Remain enabled independently of update-PR generation. Dependabot security-update PR generation is separately managed in repository settings and was confirmed disabled at cutover. |
| CI lane selection | `tools/ci-impact.ts` and the required CI workflow | Dependency manifests and lockfiles select the existing affected quality lanes. Unknown changes fall back to the full lane set. A Deno lockfile change selects all lanes. |
| Accepted exceptions | This page | Record the reason, update trigger, and required verification here; do not use this page as a second version source. |

The monthly `Dependency Review` workflow and the matching local commands only
report candidates. They do not edit dependency files, create branches, or
open pull requests. CI caches and runner environments are rebuildable work;
they are not dependency authorities.

Renovate groups updates by manager: `cargo-workspace`, `deno-workspace`,
`github-actions`, `docker`, `devcontainers`, `npm`, `mise-tools`,
`rust-toolchain`, `renovate-cli`, and `renovate-config`. Dockerfile and Docker
Compose updates share the `docker` group. The five Deno manifests and root
`deno.lock` share `deno-workspace`. The daily Actions run evaluates enabled
managers; `renovate.json` has no manager-specific schedule or Dashboard
approval gate. Eligible updates are proposed immediately with no concurrent
or hourly cap. Automatic merging remains disabled. Security alerts remain
separate from the routine ecosystem groups. The final vulnerability-alert rule
keeps routine compatibility holds and `allowedVersions` restrictions from
suppressing security-fix candidates; those PRs still require the existing CI,
CodeQL, review, and merge-queue checks.

## Deno grouped update status

The five Deno manifests share the root `deno.lock`; the `deno-workspace` group
proposes eligible updates together without Dashboard approval. Generated PR
#5115 includes all five manifests and the shared lockfile. It is checked by the
existing Web, Docsite, artifact, Rust, E2E, and CodeQL lanes; its current head
passed those checks. The source SHA and run are recorded below. The old
Dependabot Deno PR #5082 was closed after the writer switch. Track final
generated-candidate evidence in
[#5100](https://github.com/ugoite/ugoite/issues/5100).

The App-backed inventory covers all five Deno manifests. The successful
extract, lookup, and full dry-runs do not prove a generated Deno PR passes CI.
See the run record below for the exact sources and limits of each result.

## Intentionally held updates and security exceptions

| Target | Current held state | Reason | Revisit trigger and verification |
| --- | --- | --- | --- |
| Arrow / Parquet / Iceberg / DataFusion / OpenDAL stack | Renovate excludes Cargo major/minor updates with `enabled: false`; routine patches remain eligible for `cargo-workspace`. | The stack shares concrete Arrow types; a single-crate major or minor update can break compatibility across the stack. | Re-enable only after a coordinated candidate is designed and reviewed; validate the crates, Iceberg adapters, SQL, and storage regressions together. Review quarterly. |
| `keyring` major updates | Renovate excludes routine Cargo major updates for `keyring`; current-major updates and vulnerability-alert fixes remain eligible. | Cargo workspace PR #5119 initially could not regenerate `Cargo.lock` for `keyring` 4 because it no longer exposes the `apple-native` feature requested by `crates/ugoite-cli/Cargo.toml`. After the major hold, the regenerated candidate leaves `keyring` at v3 and includes `Cargo.lock`. | Revisit after the CLI removes or replaces that feature; validate `cargo check --workspace --locked` and `cargo test --workspace --locked`. Track candidate verification in issue #5121. |
| `rig-agent` minor and major updates | Renovate excludes non-vulnerability minor/major updates; vulnerability-alert updates remain eligible. | Cargo workspace candidate #5119 updates `rig-agent` 0.44.0 to 0.47.0, whose API no longer matches `crates/ugoite-konase-rig/src/lib.rs`. The adapter is deliberately exact-pinned because it lacks behavior tests for this migration. | Revisit in a dedicated adapter migration with behavioral tests, then run the affected Rust lanes and grouped Cargo CI. Track current candidate evidence in [#5147](https://github.com/ugoite/ugoite/issues/5147). |
| `sqlparser` minor and major updates | Renovate excludes non-vulnerability minor/major updates; vulnerability-alert updates remain eligible. | `ugoite-domain` candidate 0.64.0 and the pinned Iceberg/DataFusion path on 0.62.0 expose distinct AST types at the `ugoite-iceberg` boundary; candidate CI fails to compile the adapter. | Revisit only when DataFusion and Iceberg can move to a compatible parser version in a coordinated update. Validate SQL and storage checks together. Track the AST migration in [#5144](https://github.com/ugoite/ugoite/issues/5144) and candidate in [#5147](https://github.com/ugoite/ugoite/issues/5147). |
| `p256` minor and major updates | Renovate excludes non-vulnerability Cargo updates; vulnerability-alert updates remain eligible. | The regenerated candidate #5119 moves `p256` from 0.13.2 to 0.14.0. CI run [#45330847246](https://github.com/ugoite/ugoite/actions/runs/45330847246) fails in Rust check, Rust test, and artifact build because `crates/ugoite-cli/src/commands/auth.rs` uses the removed `OsRng` import path and APIs that now require `Generate` and an updated encoded-point trait. | Revisit after a reviewed authentication adapter migration; validate the auth code, workspace Rust check/test, and grouped Cargo CI. Track the policy hold in [#5159](https://github.com/ugoite/ugoite/issues/5159) and regenerated candidate verification in [#5147](https://github.com/ugoite/ugoite/issues/5147). |
| GitHub Actions Node major updates | Renovate excludes routine `node` major updates for the `github-actions` manager; vulnerability-alert updates remain eligible. The npm `engines.node >=20` package support floor remains a separate contract. | Grouped Actions PR #5117 changed `actions/setup-node` from Node 24 to 26. CI run [#45331135864](https://github.com/ugoite/ugoite/actions/runs/45331135864) failed `tools/workspace_test.ts` because the workflow runtime no longer matched the CI toolchain authority. | Revisit after an intentional CI runtime change updates both workflow configuration and its authority check; regenerate the grouped Actions candidate and pass required CI. Track the hold in [#5161](https://github.com/ugoite/ugoite/issues/5161) and candidate evidence in [#5158](https://github.com/ugoite/ugoite/issues/5158). |
| `iceberg` and `iceberg-datafusion` Git dependencies | Revision `1ecd497` in `crates/ugoite-iceberg/Cargo.toml`. | This upstream area is consumed by Git revision, not a normal registry version update. | Review a compatible upstream revision and verify saved-Space reopen, bounded queries, storage, and API behavior monthly. |
| Mitase bootstrap | Version `0.2.5` and four target SHA-256 values in `tools/mitase.lock.toml`. | Bootstrap and Mitase contracts depend on the exact downloaded bytes. | Update version and all four checksums together; run bootstrap and Mitase contract checks monthly. |
| MinIO CI image | `pgsty/minio@sha256:2b36182f3479c58b5cba920f20479738ee85ce218de0596a244f9a1368268db9` in `.github/workflows/ci.yml`. | The S3 authorization scenario uses a specific image build. | For a new image digest, rerun the S3/ACL scenario and the affected CI lane. Review monthly. |
| sccache action | `mozilla-actions/sccache-action` in `.github/workflows/ci.yml` is pinned to commit `fc920bf0ec8de6ee65d409111f7ec508035751ba` (upstream tag comment `v0.0.11`). The hosted runner reported sccache `0.17.0`. | The action and cache behavior affect CI execution. The action revision, rather than an independently installed binary version, is pinned. | On an action or runner change, verify cache setup and affected CI lanes. Review monthly. |
| TypeScript major updates | Renovate excludes major updates with `enabled: false`; patch and minor updates remain eligible. | TypeScript 7 removes compiler APIs used by `tools/check_frontend_changed_types.ts`. | Re-enable major updates after that consumer supports the new APIs; validate frontend changed-file type checking. |
| Starlight | Renovate holds `0.42.0`–`0.42.11` and `0.43.0`–`0.43.1` in `renovate.json`. | Starlight `0.42.0` failed static rendering in grouped Deno candidates with Astro `7.4.12` and `7.4.13`; Deno could not resolve `astro:middleware`. | Revisit a later candidate; verify Docsite build and navigation tests. |
| Playwright | Renovate holds `1.61.0`, `1.61.1`, `1.62.0`, `1.62.1`, `1.63.0`, `1.64.0`, `1.64.1`, `1.65.0`, and `1.65.1`. | The held targets resolved to `1.65.1` and Docsite navigation tests loaded as CommonJS, where `import.meta` failed. | Revisit with a candidate that preserves module loading; verify Docsite navigation and E2E browser setup. |
| Ubuntu GitHub Actions runner | Renovate excludes the observed `ubuntu` 26.04 runner target. | Grouped Actions PR #5117 selected `ubuntu-26.04`; CI run #45314368028 reports that Playwright does not support Chromium on `ubuntu26.04-x64`, so browser installation and its dependent lanes fail. | Revisit after the pinned Playwright browser tooling supports that runner; verify browser installation, Docsite navigation, and the required E2E lanes. Track the candidate in [#5148](https://github.com/ugoite/ugoite/issues/5148). |
| SolidStart | Renovate holds `2.0.0`–`2.0.6`, `2.1.0`, and `2.2.0`–`2.2.3`; `1.5.1` remains eligible. | The held targets resolved to `2.2.3`, which removes `defineConfig` used by `frontend/app.config.ts`. | Revisit a compatible candidate; verify frontend type checking, tests, and build. |
| SolidJS runtime and router | Renovate holds major updates of `solid-js` and `@solidjs/router`; updates within the current major remain eligible. | Grouped Deno PR #5115 resolved SolidJS `2.0.3`, which does not export `solid-js/web` used by the frontend, and its Web tests failed on changed locale/context behavior. Earlier grouped Deno candidates recorded the same missing export in issue #5045. | Re-enable the major updates after the frontend supports the new exports and runtime behavior; verify the Web type check, tests, and build. Track candidate evidence in [#5045](https://github.com/ugoite/ugoite/issues/5045). |
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
| First live writer run | [run 45191937781](https://github.com/ugoite/ugoite/actions/runs/45191937781), cutover SHA `e7a3006bed006b5ad1976431930a725ef46ea0c4` | Success; App alert access succeeded and Renovate created [Dependency Dashboard #5099](https://github.com/ugoite/ugoite/issues/5099). This pre-grouping run created no update PRs. |
| Daily grouped writer run | [run 45319646701](https://github.com/ugoite/ugoite/actions/runs/45319646701), source SHA `6d51975cfb42e660d06b04deb01a28ce816435c8` | Success; the intended App identity check and Dependabot alerts API check passed. Dashboard #5099 had zero routine updates awaiting approval and zero awaiting schedule. The run created grouped candidates #5115–#5119. |
| Latest main CI baseline | [run #45322845599](https://github.com/ugoite/ugoite/actions/runs/45322845599), main SHA `1eea649bc71a018e60c93a5b7f04314fa5d28dbe` | All planned CI lanes and `ci-required` passed on the current main baseline. This does not replace candidate-specific CI. |
| Deno workspace candidate | [PR #5115](https://github.com/ugoite/ugoite/pull/5115), head `3a0ef8a4b7d2e6e18064747a2d44f00f06fad0e1`; [CI run #45323095331](https://github.com/ugoite/ugoite/actions/runs/45323095331) | Includes all five Deno manifests and root `deno.lock`. The required CI lanes and CodeQL completed successfully for this candidate head. |
| GitHub Actions candidate | [PR #5117](https://github.com/ugoite/ugoite/pull/5117); [first CI run #45314368028](https://github.com/ugoite/ugoite/actions/runs/45314368028); [regenerated-head CI run #45331135864](https://github.com/ugoite/ugoite/actions/runs/45331135864) | The first candidate selected `ubuntu-26.04`, which Playwright did not support; that runner target is held. The regenerated head retained the supported runner but changed Node 24 to 26, failing the toolchain authority assertion in `tools/workspace_test.ts`. Its mobile E2E run also had a separate Saved SQL editor mismatch in case A07 (the expected query was followed by `SELECT 1`); this is tracked separately and does not establish a Node-related E2E failure. The Node major hold is tracked in [#5161](https://github.com/ugoite/ugoite/issues/5161); the Saved SQL mismatch still needs classification before the grouped candidate is considered verified. |
| Cargo workspace candidate | [PR #5119](https://github.com/ugoite/ugoite/pull/5119); [CI run #45322126633](https://github.com/ugoite/ugoite/actions/runs/45322126633) | The `keyring` hold allowed Renovate to generate a consistent `Cargo.lock`, but `rig-agent` 0.47.0 and `sqlparser` 0.64.0 fail against current adapter boundaries. Those exact routine updates are held pending a regenerated candidate and required CI. |
| Other grouped candidates | [Dev Containers #5116](https://github.com/ugoite/ugoite/pull/5116), [mise #5118](https://github.com/ugoite/ugoite/pull/5118) | Both candidates' required CI and CodeQL checks passed on their current heads; neither is approved or merged by this record. |
| Writer attribution follow-up | [#5120](https://github.com/ugoite/ugoite/issues/5120) | PR #5115 received a head update at 03:58 UTC to `3a0ef8a4`; the GitHub timeline attributes the force-push event to `renovate[bot]`, but no matching `.github/workflows/renovate.yml` run appears at that timestamp. The source of this update remains unverified, so single-writer completion is not claimed. |
| Earlier update candidates | PRs [#5112](https://github.com/ugoite/ugoite/pull/5112), [#5113](https://github.com/ugoite/ugoite/pull/5113), and [#5114](https://github.com/ugoite/ugoite/pull/5114) | They remain open pending issue-link and Rust verification follow-up in #5120 and #5125. No failing dependency update was merged. |

The extract, lookup, full dry-run, and live run reported that Renovate could not
determine a digest for the manually managed `iceberg-rust` Git tag because the
upstream tag has a `v` prefix. Earlier full dry-run and live runs also logged
four Cargo/Docker grouped-update collisions. These diagnostics were non-fatal
in each run in which they appeared. The Iceberg revision remains manually
managed; do not mark these diagnostics as a passed dependency update.

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
