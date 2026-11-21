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
Cargo's Arrow stack also keeps its per-ecosystem group for eligible patch
updates. Rust toolchain updates remain monthly.

## Deno update grouping evidence

The five Deno manifests share the root `deno.lock`. Dependabot monitors those
directories in one weekly wildcard version-update group with
`open-pull-requests-limit: 1`. Generated PRs #4994 and #5000 each included all
five manifests and the shared lockfile; #5000's title says one directory, but
its diff covers the full workspace. It was the only open Deno version-update
PR after the earlier duplicate #4995 was closed. Exact compatibility holds
are listed below; security updates remain independently visible.

The first grouped update after the multi-directory policy,
[PR #4994](https://github.com/ugoite/ugoite/pull/4994), included all five
manifests and the shared lockfile. Frozen dependency verification passed, but
required CI rejected three candidate/resolution pairs:

- Starlight `0.42.11` failed Docsite static rendering because Deno could not
  resolve `astro:middleware`.
- Playwright target `1.64.1` resolved to `1.65.1` in the lockfile; Docsite
  navigation tests then loaded as CommonJS and failed on `import.meta`.
- SolidStart target `2.2.2` resolved to `2.2.3` in the lockfile; frontend type
  checking reported the missing `defineConfig` export.

[PR #4995](https://github.com/ugoite/ugoite/pull/4995) was titled as a
one-directory update but changed the same five manifests and shared lockfile
as #4994. It was closed as a duplicate after the manifest diffs were compared.
The exact target and resolved versions from #4994 were added to the holds by
[PR #4999](https://github.com/ugoite/ugoite/pull/4999), preserving the
existing group policy and the earlier Starlight `0.43.0` / `0.43.1` and
Playwright `1.65.0` / `1.65.1` observations recorded below.

After #4999, Dependabot generated [PR #5000](https://github.com/ugoite/ugoite/pull/5000).
Its frozen install passed, but required CI reproduced the same incompatibilities
with lower manifest targets: Starlight `0.42.10` failed static rendering on
`astro:middleware`; Playwright `1.64.0` resolved to `1.65.1` and Docsite
navigation failed on `import.meta` outside a module; SolidStart `2.2.1`
resolved to `2.2.3` and frontend type checking reported missing `defineConfig`.
These exact targets were added to the existing holds by
[PR #5002](https://github.com/ugoite/ugoite/pull/5002). Dependabot closed
#5000 after those holds landed; that closure is not evidence that the remaining
updates passed.

After #5002, Dependabot generated
[PR #5005](https://github.com/ugoite/ugoite/pull/5005), which reports five
directories and 63 updates. Its diff contains all five Deno manifests and the
shared `deno.lock`; it was the only open Deno update PR. The frozen
`deps:deno:verify` task passed. Required CI then rejected three candidates:

- Starlight `0.42.9` failed Docsite static rendering because Deno could not
  resolve `astro:middleware`.
- Playwright target `1.63.0` resolved to `1.65.1`; Docsite navigation tests
  loaded as CommonJS and failed on `import.meta` outside a module.
- SolidStart target `2.2.0` resolved to `2.2.3`; frontend type checking
  reported that `defineConfig` is missing from the package export.

Rust, S3 authorization, and CP1 fixture lanes passed; `ci-required` failed
because of the three affected lanes. The exact candidates were added to the
existing holds by [PR #5007](https://github.com/ugoite/ugoite/pull/5007).
Dependabot closed #5005 after those holds merged; that closure does not show
that its remaining updates passed.

[PR #5010](https://github.com/ugoite/ugoite/pull/5010), generated after
#5007, reported one directory and 63 updates. Its diff included all five Deno
manifests and the shared `deno.lock`. The frozen `deps:deno:verify` task passed
in Web and artifact preparation. Required CI run
[44880629751](https://github.com/ugoite/ugoite/actions/runs/44880629751)
rejected three candidates:

- Starlight `0.42.8` failed Docsite static rendering because Deno could not
  resolve `astro:middleware`.
- Playwright target `1.62.1` resolved to `1.65.1`; Docsite navigation tests
  loaded as CommonJS and failed on `import.meta` outside a module.
- SolidStart target `2.1.0` resolved to `2.2.3`; frontend type checking
  reported that `defineConfig` is missing from the package export.

Rust check/test, S3 authorization, and CP1 fixture lanes passed. E2E lanes were
skipped after required lanes failed, and `ci-required` failed. The exact
targets were added to the existing holds by
[PR #5012](https://github.com/ugoite/ugoite/pull/5012). Dependabot closed
#5010 after those holds merged; that closure does not show that its remaining
updates passed.

[PR #5017](https://github.com/ugoite/ugoite/pull/5017), generated after
#5012, reports five directories and 63 updates. Its diff contains all five
Deno manifests and the shared `deno.lock`, and it was the only open Deno
version-update PR observed. The frozen `deps:deno:verify` task passed in the
Web and artifact-preparation jobs; Docsite navigation did not run that task.
Required CI then rejected three candidates:

- Starlight `0.42.7` failed Docsite static rendering because Deno could not
  resolve `astro:middleware`.
- Playwright target `1.62.0` resolved to `1.65.1`; Docsite navigation tests
  loaded as CommonJS and failed on `import.meta` outside a module.
- SolidStart target `2.0.6` resolved to `2.2.3`; frontend type checking
  reported that `defineConfig` is missing from the package export.

Rust check/test, S3 authorization, and CP1 fixture lanes passed. E2E and CP1
query/export were skipped after required lanes failed, and `ci-required`
failed. The exact targets were added to the existing holds by
[PR #5022](https://github.com/ugoite/ugoite/pull/5022). Dependabot closed
#5017 after those holds merged; that closure does not show that its remaining
updates passed.

[PR #5029](https://github.com/ugoite/ugoite/pull/5029), generated after
#5022, reported one directory and 63 updates. Its diff nevertheless included
all five Deno manifests and the shared `deno.lock`, and it was the only open
Deno update PR. The frozen `deps:deno:verify` task passed in Web and artifact
preparation; Docsite navigation did not run that task. Required CI run
[44888354206](https://github.com/ugoite/ugoite/actions/runs/44888354206)
reproduced the three compatibility failures with lower manifest targets:

- Starlight `0.42.6` failed Docsite static rendering because Deno could not
  resolve `astro:middleware`.
- Playwright target `1.61.1` resolved to `1.65.1`; Docsite navigation tests
  loaded as CommonJS and failed on `import.meta` outside a module.
- SolidStart target `2.0.5` resolved to `2.2.3`; frontend type checking
  reported that `defineConfig` is missing from the package export.

Rust check/test, S3 authorization, and CP1 fixture lanes passed. E2E and CP1
query/export were skipped after required lanes failed, and `ci-required`
failed. CodeQL run
[44888354142](https://github.com/ugoite/ugoite/actions/runs/44888354142)
passed. The exact targets were added to the existing holds by
[PR #5031](https://github.com/ugoite/ugoite/pull/5031); Dependabot closed
#5029 after that policy change merged. That closure is not evidence that the
remaining updates passed.

[PR #5033](https://github.com/ugoite/ugoite/pull/5033), generated after
#5031, reports five directories and 63 updates. Its diff changes all five
Deno manifests and the shared `deno.lock`, and it is the only open Deno
version-update PR. The frozen `deps:deno:verify` task passed in Web and artifact
preparation; Docsite navigation did not run that task. Required CI run
[44891845573](https://github.com/ugoite/ugoite/actions/runs/44891845573)
reproduced the three compatibility failures with lower manifest targets:

- Starlight `0.42.5` failed Docsite static rendering because Deno could not
  resolve `astro:middleware`.
- Playwright target `1.61.0` resolved to `1.65.1`; Docsite navigation tests
  loaded as CommonJS and failed on `import.meta` outside a module.
- SolidStart target `2.0.4` resolved to `2.2.3`; frontend type checking
  reported that `defineConfig` is missing from the package export.

`ci-impact-plan`, `ci-pr-context-report`, Rust check, S3 authorization, and
CP1 fixture and Rust test lanes passed. E2E and CP1 query/export were skipped
after required lanes failed, and `ci-required` failed. CodeQL run
[44891845615](https://github.com/ugoite/ugoite/actions/runs/44891845615)
passed. The exact new target holds were added by
[PR #5037](https://github.com/ugoite/ugoite/pull/5037). Dependabot closed
#5033 after that policy change merged; that closure is not evidence that
remaining candidates passed.

[Issue #4693](https://github.com/ugoite/ugoite/issues/4693) tracks verification
of a grouped update. Workspace coverage, the one-open-PR limit, and the frozen
boundary have been exercised by generated update PRs, but the observed grouped
updates have not passed their required CI lanes. The rollout remains incomplete
until a grouped PR generated after the latest holds has no duplicate candidate
and passes required CI. Compare generated changes before closing duplicates;
close remaining candidates only after their updates are accounted for in a
successful grouped PR.

[PR #5040](https://github.com/ugoite/ugoite/pull/5040), generated after #5037,
reported five directories and 62 updates. Its diff included all five Deno
manifests and the shared `deno.lock`. Frozen verification passed in Web and
artifact preparation. Required CI rejected Starlight target `0.42.4` because
Docsite static rendering could not resolve `astro:middleware`, and SolidStart
target `2.0.3` resolved to `2.2.3`, whose missing `defineConfig` export failed
frontend type checking. The exact targets were added to the existing holds by
[PR #5042](https://github.com/ugoite/ugoite/pull/5042). Dependabot closed #5040
after that policy change; that closure is not evidence that remaining updates
passed.

[PR #5048](https://github.com/ugoite/ugoite/pull/5048), generated after #5042,
also reported five directories and 62 updates. Its diff included all five Deno
manifests and the shared `deno.lock`; it was the only open Deno version-update
PR while open. Frozen verification passed in Web and artifact preparation.
Required CI rejected Starlight target `0.42.3` because Docsite static
rendering could not resolve `astro:middleware`, and SolidStart target `2.0.2`
resolved to `2.2.3`, whose missing `defineConfig` export failed frontend type
checking. Rust check/test, S3 authorization, CP1 fixtures, and CodeQL passed;
E2E and CP1 query/export were skipped after required lanes failed, so
`ci-required` failed. The exact targets were added to the existing holds by
[PR #5050](https://github.com/ugoite/ugoite/pull/5050). Dependabot closed #5048
after that policy change; that closure is not evidence that remaining updates
passed.

After #5050 merged at `c971889`, comparison runs
[44903552130](https://github.com/ugoite/ugoite/actions/runs/44903552130) for
the five configured directories and
[44903556271](https://github.com/ugoite/ugoite/actions/runs/44903556271) for a
root-only configuration were started from the same main commit. Their generated
update and validation evidence is tracked in [issue #5045](https://github.com/ugoite/ugoite/issues/5045).
This page does not treat run launch or a policy-triggered bot PR closure as a
successful update; keep #4693 open until a generated update satisfies its
acceptance criteria.

## Intentionally held updates and security exceptions

| Target | Current held state | Reason | Revisit trigger and verification |
| --- | --- | --- | --- |
| Arrow / Parquet / Iceberg / DataFusion / OpenDAL stack | Dependabot suppresses major and minor updates; patch updates remain eligible and grouped. | The stack shares concrete Arrow types; a single-crate major or minor update can break compatibility across the stack. | Propose a compatible stack update; validate the coordinated crates, Iceberg adapters, SQL, and storage regressions together. Review quarterly. |
| `iceberg` and `iceberg-datafusion` Git dependencies | Revision `1ecd497` in `crates/ugoite-iceberg/Cargo.toml`. | This upstream area is consumed by Git revision, not a normal registry version update. | Review a compatible upstream revision and verify saved-Space reopen, bounded queries, storage, and API behavior monthly. |
| Mitase bootstrap | Version `0.2.5` and four target SHA-256 values in `tools/mitase.lock.toml`. | Bootstrap and Mitase contracts depend on the exact downloaded bytes. | Update version and all four checksums together; run bootstrap and Mitase contract checks monthly. |
| MinIO CI image | `pgsty/minio@sha256:2b36182f3479c58b5cba920f20479738ee85ce218de0596a244f9a1368268db9` in `.github/workflows/ci.yml`. | The S3 authorization scenario uses a specific image build. | For a new image digest, rerun the S3/ACL scenario and the affected CI lane. Review monthly. |
| sccache action | `mozilla-actions/sccache-action` in `.github/workflows/ci.yml` is pinned to commit `fc920bf0ec8de6ee65d409111f7ec508035751ba` (upstream tag comment `v0.0.11`). The hosted runner reported sccache `0.17.0`. | The action and cache behavior affect CI execution. The action revision, rather than an independently installed binary version, is pinned. | On an action or runner change, verify cache setup and affected CI lanes. Review monthly. |
| TypeScript major updates | Dependabot ignores semver-major updates for `typescript`; patch and minor updates remain eligible. | TypeScript 7 removes compiler APIs used by `tools/check_frontend_changed_types.ts`. | Remove the ignore after that consumer supports the new APIs; validate frontend changed-file type checking. |
| Starlight | Dependabot holds targets `0.42.3`, `0.42.4`, `0.42.5`, `0.42.6`, `0.42.7`, `0.42.8`, `0.42.9`, `0.42.10`, `0.42.11`, `0.43.0`, and `0.43.1`. | Each observed grouped candidate failed static rendering because Deno could not resolve `astro:middleware`. | Revisit when a candidate is available; verify Docsite build and navigation tests. |
| Playwright | Dependabot holds targets `1.61.0`, `1.61.1`, `1.62.0`, `1.62.1`, `1.63.0`, `1.64.0`, `1.64.1`, and `1.65.0`, plus resolved version `1.65.1`. | The targets resolved to `1.65.1` and Docsite navigation tests loaded as CommonJS, where `import.meta` failed. | Revisit with a candidate that preserves module loading; verify Docsite navigation and E2E browser setup. |
| SolidStart | Dependabot holds targets `2.0.2`, `2.0.3`, `2.0.4`, `2.0.5`, `2.0.6`, `2.1.0`, `2.2.0`, `2.2.1`, and `2.2.2`, plus resolved version `2.2.3`. | The targets resolved to `2.2.3`, which removes `defineConfig` used by `frontend/app.config.ts`. | Revisit when a compatible candidate is available; verify frontend type checking, tests, and build. |
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
