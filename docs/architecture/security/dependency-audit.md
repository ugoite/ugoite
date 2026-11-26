---
title: Dependency audit exceptions
---

This page records update exceptions, their reasons, and the evidence needed to
revisit them. It is not a version authority: manifests, lockfiles, toolchain
files, and workflow configuration below decide the actual versions and update
policy.

## Operational status snapshot — 2026-11-27 JST / 2026-11-26 UTC

Latest main is `0f36c02a9711dc0746a47a5e24e3f00c96c53bab`, merging Renovate CLI 45.0.1 PR #5301 through the normal merge queue. Main CI [#45872735078](https://github.com/ugoite/ugoite/actions/runs/45872735078), CodeQL [#45872735029](https://github.com/ugoite/ugoite/actions/runs/45872735029), and Code Quality [#45872735131](https://github.com/ugoite/ugoite/actions/runs/45872735131) passed. No release, tag, publish, or product-version change occurred.

The eight original ecosystem update PRs and Docker authority #5179, Rust 1.100 Clippy correction #5182, Cargo policy #5204, grouped Cargo update #5278, Renovate CLI updates through #5301, release-age correction #5288, route-recovery correction #5292, Vitest WASM harness fix #5294, and jsdom event-realm test correction #5300 are merged through the normal merge queue. #5278 changed only the shared `Cargo.lock`, preserving manifest ranges and exact pins. Existing protections and Renovate's `automerge: false` remain in place.

After #5301 merged, CLI 45.0.1 read-only full diagnostic [#45872817347](https://github.com/ugoite/ugoite/actions/runs/45872817347) and self-hosted writer [#45873585621](https://github.com/ugoite/ugoite/actions/runs/45873585621) succeeded on this main. Both passed the App-slug and alerts-API access preflights and evaluated 37 package files / 630 dependencies across 10 managers, including all five Deno manifests. The diagnostic encountered Docker Hub HTTP 403 responses for `library/rust` and `denoland/deno`, so Docker lookup completeness is not established by that run. The writer found no Docker updates, refreshed App-authored Deno PR #5279 to head `81170277360860e01e024f97c9a7dbfee8794f6d`, and created App-authored PR #5302 for Renovate CLI 45.1.0 and #5303 for the Renovate GitHub Actions wrapper 47.0.0. The writer again emitted `Could not re-extract the packageFile after updating it` for root `deno.json` and `frontend/deno.json`.

Current open candidates are #5279 (Deno), #5302 (Renovate CLI), and #5303 (GitHub Actions wrapper). Dashboard #5099 lists these candidates and the Deno warnings; at this snapshot it has no Pending Approval, Awaiting Schedule, or PR Edited (Blocked) item. #5302 and #5303 have successful Web, Rust-check, and CodeQL results, while Rust-test/artifact checks are still running; neither is eligible for queue entry until its exact-head required checks finish. Their changes are limited to the Renovate CLI version pins and the two wrapper action pins, respectively.

At #5279 head `81170277360860e01e024f97c9a7dbfee8794f6d` against main `0f36c02a`, the candidate changes only Deno manifests and the shared lockfile. The `ci-web` job in [run #45873861020](https://github.com/ugoite/ugoite/actions/runs/45873861020) failed; the matching main Web job in [run #45872735078](https://github.com/ugoite/ugoite/actions/runs/45872735078) passed. At this snapshot the overall candidate run is still active in `artifact-build`, so GitHub has not made its complete job log available. Rust check/test, docsite navigation, fixture, S3-authorization, and CodeQL checks have passed; the artifact lane and its dependent checks have not completed. Do not infer the failure cause from the older candidate logs below, and do not queue this head. Issue [#5239](https://github.com/ugoite/ugoite/issues/5239) tracks the failure and re-extraction warning.

Scheduled run [#45729166783](https://github.com/ugoite/ugoite/actions/runs/45729166783) succeeded after the administrator-reported external Renovate App removal, on prior main `c83a4561afa8d35363c3c46d320a4de6f65b8df3` with CLI 44.263.1. The latest runs on `0f36c02a` are manual diagnostic/writer runs; no scheduled run on this main is observed yet. External App removal, Dependency graph/alerts enabled, and Dependabot security-update PR generation disabled remain administrator-reported; a settings API 403 does not establish those values. The App-backed alerts API preflight passes but does not enumerate alerts. The current GitHub integration cannot enumerate alert records (403), so no live patchable security-fix candidate is confirmed; track this distinction in [#4997](https://github.com/ugoite/ugoite/issues/4997).

No human review is claimed. Independent AI reviews record exact HEAD, base, scope, findings, and rationale. Existing CI, CodeQL, protections, and merge queue remain in force; Renovate automerge remains disabled.

## Authorities and verification

| Domain | Authority | Verification and notes |
| --- | --- | --- |
| Rust crates | Workspace `Cargo.toml` files and `Cargo.lock` | CI and release builds use locked Cargo operations. |
| Deno dependencies | Root and member `deno.json` files plus the single root `deno.lock` | `mise run deps:deno:verify` runs `deno install --frozen --allow-scripts=npm:@tailwindcss/oxide,npm:esbuild,npm:sharp`. `setup`, the hosted Web lane, and artifact preparation call this task. The lockfile-hash cache is disposable; the frozen install verifies the manifest-to-lockfile boundary. Deno command and source imports use manifest aliases rather than embedding a second package version. |
| Rust compiler | `rust-toolchain.toml` | The same channel and components are used by local development and CI. `mise.toml` enables mise's idiomatic Rust version file instead of duplicating a compiler pin. |
| Development tools | `mise.toml` | This file pins Deno `2.9.8`, Helm `4.3.2`, nextest `cargo-nextest-0.9.143`, and the minimum mise version `2025.11.3`. Rust follows `rust-toolchain.toml`. |
| Docker Deno runtime | `Dockerfile` | The frontend build image is explicitly pinned to `denoland/deno:2.9.8`, matching the development toolchain. |
| Node runtime | `.github/workflows/ci.yml`, `release-candidate.yml`, and `release-publish.yml` | CI and release workflows use Node 24. `packages/ugoite/package.json` declares `engines.node >=20` as the package support floor. |
| Update policy | `renovate.json` | Defines enabled managers, one group per ecosystem, compatibility holds, immediate PR creation, and the no-auto-merge policy. This is the repository's update-policy authority. |
| Update-PR runner | `.github/workflows/renovate.yml` | Runs the pinned Renovate CLI on a daily schedule and manual dispatch using a GitHub App scoped to this repository. Live evidence below records update candidates and writer identity. |
| Vulnerability alerts | GitHub Dependency graph and Dependabot alerts settings | Managed independently of update-PR generation. The Renovate workflow verifies App access to the alerts API; the separate security-update PR toggle is not exposed by that workflow evidence. |
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
`deno.lock` share `deno-workspace`. The Actions workflow runs daily at 01:17
JST (16:17 UTC on the previous day) and evaluates enabled managers together;
`renovate.json` has no manager-specific schedule or Dashboard approval gate.
Eligible updates are proposed immediately with no concurrent or hourly cap.
Automatic merging remains disabled. Security alerts remain
separate from routine ecosystem groups. The vulnerability-alert rule keeps
routine compatibility holds and `allowedVersions` restrictions from
suppressing security-fix candidates. Security candidates still require the
existing CI, CodeQL, GitHub protections, and merge-queue checks. AI review is
recorded with the exact candidate HEAD and scope; no human review is claimed.

## Cargo update-policy verification

PR #5204 adds Cargo-manager-only rangeStrategy: update-lockfile; Cargo grouping,
compatibility holds, vulnerability handling, and automerge: false remain unchanged. On
the post-policy source, Renovate regenerated grouped Cargo candidate #5119 with all 13
Cargo manifests represented. The candidate retained declared SemVer ranges for in-range
updates, changed manifest requirements when the selected update was outside the prior
range, and updated the shared lockfile. Locked Cargo metadata, candidate CI/E2E, CodeQL,
and normal merge-queue integration passed. The exact-pin sqlparser update in #5262 was
validated separately and does not by itself exercise an in-range range update. The later App-authored grouped candidate #5278 changed only `Cargo.lock`, preserved every existing `Cargo.toml` requirement, and merged through the normal queue as `bfc62ed70f4015cb3a044e2f6d674dc162a6a455` from head `854d04dc11b41b39773668be5c8632011e178529`. Exact-head CI [#45646977591](https://github.com/ugoite/ugoite/actions/runs/45646977591) passed Rust check/test, Web, artifact build, and selected E2E; CodeQL [#45646977431](https://github.com/ugoite/ugoite/actions/runs/45646977431) also passed. This is live evidence that the Cargo-only `update-lockfile` policy updates an in-range group without rewriting manifest ranges.

## Deno workspace update status — prior candidate history

The detailed failure investigation below records superseded CLI 45.0.0 candidate heads. The current 45.0.1 writer head and its still-running CI evidence are summarized in the operational snapshot above; the current Web failure is not assigned the older failures’ cause until its own logs are available.

All five Deno manifests are evaluated together and share the root `deno.lock`.
The CLI 45.0.0 read-only full dry-run [#45863927350](https://github.com/ugoite/ugoite/actions/runs/45863927350)
and self-hosted writer [#45864901396](https://github.com/ugoite/ugoite/actions/runs/45864901396)
each evaluated five Deno manifest paths in a 37-file / 630-dependency inventory.
The writer's Deno manager remains one `deno-workspace` group, and frozen
verification is `mise run deps:deno:verify`.

The earlier CLI 45.0.0 App-authored candidate [#5279](https://github.com/ugoite/ugoite/pull/5279)
was at head `a73b9025470efcd97a45b3e2a03d3932062d993f` against main
`2d53e541187743199f3710a2a511a358b32b5ec1`. It changes root `deno.json`,
`frontend/deno.json`, `e2e/deno.json`, and root `deno.lock`; `docsite/` and
`tools/` were evaluated but had no eligible updates. The candidate's frozen
install progressed into tests, which verifies that this observed failure is
not a manifest-to-lock mismatch.

On that exact head, Web job #140066798392 in
[CI run #45865229846](https://github.com/ugoite/ugoite/actions/runs/45865229846)
reported 112 failed tests across 11 files. It also records three uncaught
`EventTarget.dispatchEvent` realm/type errors at 17:11:36, 17:11:41, and
17:12:45 UTC. The first recorded unhandled request was from
`src/lib/api.test.ts > apiFetch auth forwarding > REQ-OPS-015: forwards SSR
auth headers to local auth requests` (line 54). It registers the expected MSW
`http.get(testApiUrl("/auth/config"), ...)` handler, removes `window`, then
calls `apiFetch`; the refused request reaches `fetch` at `src/lib/api.ts:96`
for `http://localhost:3000/api/auth/config`, followed by connection refusals
and `UgoiteApiError: fetch failed`. `CompositionStudioSync.test.tsx` reports
34 tests passed before the three `dispatchEvent` errors were logged, but this
timing does not identify their source or whether they match #5300's three
corrected focus-event constructions. The log does not establish whether those
event errors and unhandled API requests share a cause. The corresponding main Web job in
[run #45863869696](https://github.com/ugoite/ugoite/actions/runs/45863869696)
passed with 169/169 frontend test files and 1,386/1,386 tests. `frontend/src/test/setup.ts` and
`frontend/vitest.config.ts` are unchanged, as are the resolved versions of MSW
3.0.7, `@mswjs/interceptors` 0.45.12, Vitest 5.1.1, and jsdom 31.0.1. The
candidate changes include `node-mock-http` 1.0.4 to 1.0.5, `@solidjs/start`
1.3.2 to 1.5.1, `@testing-library/dom` 10.4.1 to 10.4.2, and
`vite-plugin-solid` 2.11.12 to 2.11.17. These are candidate deltas, not an
isolated cause. No compatibility hold, test weakening, assertion removal, or
timeout increase has been made. Keep #5279 out of the merge queue until a
reproducible cause is fixed and exact-head required checks pass.

The writer emitted a `Could not re-extract the packageFile after updating it`
warning for root `deno.json` and `frontend/deno.json`. The CLI 45.0.0 diagnostic
did not emit that exact warning, but logged a rebase after an error updating
`frontend/deno.json`. Both runs evaluated all five manifests and the writer
updated the shared-lock PR; this does not prove the warnings are harmless or
that no update candidate was omitted. The warning's effect remains open in
[#5239](https://github.com/ugoite/ugoite/issues/5239); compare extracted
package sets before and after update using the pinned CLI before declaring it
resolved.

Earlier candidate history: head `b4b28b110d2b361bf1291d56eaa2af937565e5af`
failed when a generated WASM `?init` loader fetched from the frontend test
origin. Source/test-harness PR #5294 fixed that observed path and passed its
exact-head Web, artifact, E2E, required CI, and CodeQL checks. Deno heads
`65a39b4`, `589d85c`, and `e028b69` subsequently showed the event-realm and
localhost-fetch pattern; the current main passed its matching Web lane. PR
#5292's bounded route-recovery correction is integrated and unchanged. Earlier
run details remain in the dated operational records below; no Deno candidate
has been merged based on those superseded heads.

PR [#5288](https://github.com/ugoite/ugoite/pull/5288) pinned
`frontend/deno.json` to `npm:rettime@0.11.12` with a matching lock update,
preserving the minimum-age policy. The CLI 44.264.0 dry-run selected 0.11.13
in its simulated update and the subsequent writer refreshed #5279 with that
eligible update. This shows that the age window eventually releases a
candidate; it does not resolve the extraction warning above.

## Intentionally held updates and security exceptions

| Target | Current held state | Reason | Revisit trigger and verification |
| --- | --- | --- | --- |
| Arrow / Parquet / Iceberg / DataFusion / OpenDAL stack | Renovate excludes Cargo major/minor updates with `enabled: false`; routine patches remain eligible for `cargo-workspace`. | The stack shares concrete Arrow types; a single-crate major or minor update can break compatibility across the stack. | Re-enable only after a coordinated candidate is designed and reviewed; validate the crates, Iceberg adapters, SQL, and storage regressions together. Review quarterly. |
| `keyring` major updates | Renovate excludes routine Cargo major updates for `keyring`; current-major updates and vulnerability-alert fixes remain eligible. | Cargo workspace PR #5119 initially could not regenerate `Cargo.lock` for `keyring` 4 because it no longer exposes the `apple-native` feature requested by `crates/ugoite-cli/Cargo.toml`. After the major hold, the regenerated candidate leaves `keyring` at v3 and includes `Cargo.lock`. | Revisit after the CLI removes or replaces that feature; validate `cargo check --workspace --locked` and `cargo test --workspace --locked`. Track candidate verification in issue #5121. |
| `rig-agent` minor and major updates | Renovate excludes non-vulnerability minor/major updates; vulnerability-alert updates remain eligible. | An earlier Cargo candidate proposed `rig-agent` 0.47.0, whose API no longer matches `crates/ugoite-konase-rig/src/lib.rs`. The refreshed candidate retains 0.44.0 under the current hold. The adapter is deliberately exact-pinned because it lacks behavior tests for this migration. | Revisit in a dedicated adapter migration with behavioral tests, then run the affected Rust lanes and grouped Cargo CI. Track the adapter migration and grouped-candidate retest in [#5242](https://github.com/ugoite/ugoite/issues/5242). |
| sqlparser minor and major updates | The targeted non-vulnerability hold is removed; vulnerability-alert updates remain eligible. | Source compatibility PR #5249 moved parser AST types behind the domain boundary. Grouped Cargo candidate #5262 updated the exact domain pin from 0.62.0 to 0.64.0 while retaining 0.62.0 for DataFusion in the shared lockfile. Its exact-head Rust test suite (including SQL form-reference alias coverage), Web, S3, artifact-build, Composition Studio E2E, required CI, and CodeQL checks passed; it merged through the normal queue at c3ba4e1c7d5ca342bbbce0b731f9382c0048a55e. | Compatibility migration is integrated. Keep routine sqlparser updates eligible and validate future grouped candidates against locked workspace resolution and query-context alias tests. Issue #5144 is resolved.
| `p256` direct major/minor updates | The non-vulnerability hold was removed by #5245; vulnerability-alert updates remain eligible. | Source compatibility PR #5235 merged before the hold was removed. App-authored grouped Cargo PR #5248 then updated `p256` to 0.14.0 and the shared `Cargo.lock`; it passed required CI, CodeQL, selected E2E, independent AI review, and the normal merge queue. The update is now in main. | The compatibility hold is resolved. Issue [#5159](https://github.com/ugoite/ugoite/issues/5159) is closed. |
| `rmcp` non-vulnerability minor updates | Renovate holds Cargo minor updates; patch and vulnerability-alert updates remain eligible. | Candidate #5119 updated `rmcp` 3.7.0 to 3.11.0 and failed `rmcp_transport_error_keeps_status_and_body_in_the_cli_error_chain` in [CI #45349053433](https://github.com/ugoite/ugoite/actions/runs/45349053433). Inspection of rmcp 3.11 shows its public HTTP status error is body-free; retaining the response body needs a custom transport. Retrying the mutating `ugoite.undo` operation could duplicate the request, and removing the assertion would weaken the test. | Keep the targeted hold unless a safe transport change is designed; periodically re-test a compatible candidate with the focused Rust test and grouped Cargo CI. Track a safe transport design and the next compatibility retest in [#5243](https://github.com/ugoite/ugoite/issues/5243). |
| GitHub Actions Node major updates | Renovate excludes routine GitHub Actions updates with `depName: node`; vulnerability-alert updates remain eligible. The npm `engines.node >=20` package support floor remains a separate contract. | Grouped Actions PR #5117 initially changed `actions/setup-node` from Node 24 to 26. CI run [#45331135864](https://github.com/ugoite/ugoite/actions/runs/45331135864) failed `tools/workspace_test.ts` because the workflow runtime no longer matched the CI toolchain authority. Renovate 44.257.9 extracts this input as `depName: node` and `packageName: actions/node-versions`; the initial package-name matcher did not apply. The matcher correction merged in PR [#5164](https://github.com/ugoite/ugoite/pull/5164). The refreshed candidate no longer changes Node 24 to 26; see current evidence below. | Revisit after an intentional CI runtime change updates both workflow configuration and its authority check; regenerate the grouped Actions candidate and pass required CI. Matcher correction is tracked in [#5163](https://github.com/ugoite/ugoite/issues/5163); refreshed-candidate evidence remains in [#5158](https://github.com/ugoite/ugoite/issues/5158). |
| `iceberg` and `iceberg-datafusion` Git dependencies | Revision `1ecd497` in `crates/ugoite-iceberg/Cargo.toml`. | This upstream area is consumed by Git revision, not a normal registry version update. | Review a compatible upstream revision and verify saved-Space reopen, bounded queries, storage, and API behavior monthly. |
| Mitase bootstrap | Version `0.2.5` and four target SHA-256 values in `tools/mitase.lock.toml`. | Bootstrap and Mitase contracts depend on the exact downloaded bytes. | Update version and all four checksums together; run bootstrap and Mitase contract checks monthly. |
| MinIO CI image | `pgsty/minio@sha256:2b36182f3479c58b5cba920f20479738ee85ce218de0596a244f9a1368268db9` in `.github/workflows/ci.yml`. | The S3 authorization scenario uses a specific image build. | For a new image digest, rerun the S3/ACL scenario and the affected CI lane. Review monthly. |
| sccache action | `mozilla-actions/sccache-action` in `.github/workflows/ci.yml` is pinned to commit `fc920bf0ec8de6ee65d409111f7ec508035751ba` (upstream tag comment `v0.0.11`). The hosted runner reported sccache `0.17.0`. | The action and cache behavior affect CI execution. The action revision, rather than an independently installed binary version, is pinned. | On an action or runner change, verify cache setup and affected CI lanes. Review monthly. |
| TypeScript major updates | Renovate excludes major updates with `enabled: false`; patch and minor updates remain eligible. | TypeScript 7 removes compiler APIs used by `tools/check_frontend_changed_types.ts`. | Re-enable major updates after that consumer supports the new APIs; validate frontend changed-file type checking. |
| Starlight | Renovate holds `0.42.0`–`0.42.11` and `0.43.0`–`0.43.1` in `renovate.json`. | Starlight `0.42.0` failed static rendering in grouped Deno candidates with Astro `7.4.12` and `7.4.13`; Deno could not resolve `astro:middleware`. Issue [#5124](https://github.com/ugoite/ugoite/issues/5124) records the failure and the hold. | Revisit a later candidate; verify Docsite build and navigation tests before removing this hold. |
| Playwright | Renovate holds `1.61.0`, `1.61.1`, `1.62.0`, `1.62.1`, `1.63.0`, `1.64.0`, `1.64.1`, `1.65.0`, and `1.65.1`. | The held targets resolved to `1.65.1` and Docsite navigation tests loaded as CommonJS, where `import.meta` failed. | Revisit with a candidate that preserves module loading; verify Docsite navigation and E2E browser setup. |
| Ubuntu GitHub Actions runner | Renovate excludes the observed `ubuntu` 26.04 runner target. | Grouped Actions PR #5117 selected `ubuntu-26.04`; CI run #45314368028 reports that Playwright does not support Chromium on `ubuntu26.04-x64`, so browser installation and its dependent lanes fail. | Revisit after the pinned Playwright browser tooling supports that runner; verify browser installation, Docsite navigation, and the required E2E lanes. Track the candidate in [#5148](https://github.com/ugoite/ugoite/issues/5148). |
| SolidStart | Renovate holds `2.0.0`–`2.0.6`, `2.1.0`, and `2.2.0`–`2.2.3`; `1.5.1` remains eligible. | The held targets resolved to `2.2.3`, which removes `defineConfig` used by `frontend/app.config.ts`. | Revisit a compatible candidate; verify frontend type checking, tests, and build. |
| SolidJS runtime and router | Renovate holds major updates of `solid-js` and `@solidjs/router`; updates within the current major remain eligible. | An earlier grouped Deno PR #5115 resolved SolidJS `2.0.3`, which does not export `solid-js/web` used by the frontend, and its Web tests failed on changed locale/context behavior. The refreshed Deno candidate remains on the current major. Earlier grouped Deno candidates recorded the same missing export in issue #5045. | Re-enable the major updates after the frontend supports the new exports and runtime behavior; verify the Web type check, tests, and build. Track candidate evidence in [#5045](https://github.com/ugoite/ugoite/issues/5045). |
| RustSec `rsa` advisory | Main contains `rsa 0.9.10`; Cargo candidate #5119 also resolves `rsa 0.10.0-rc.19`. Both versions are reported for `RUSTSEC-2023-0071`. | The 0.9.10 crate is transitive through the OIDC adapter and OpenDAL cloud backends. The 0.10.0 release candidate comes through WebAuthn's `crypto-glue` dependency. | Run `cargo audit --json` from the workspace root on graph changes and `cargo metadata --locked --format-version 1` to verify full lockfile resolution. Keep the finding visible until a patched, supported graph is available. |
| Dev Container and OS packages | Base image and feature pins are in `.devcontainer/devcontainer.json`. | Base image and OS package updates are periodic environment changes rather than application dependency updates. | Review with a base-image update; verify container setup and the basic contributor workflow. |

The frontend manifest declares `vitest` and `@vitest/coverage-v8` as `^5.0.5`,
with the `vitest/globals` alias at `^5.1.1`; the shared lock resolves each to
5.1.1. Docsite pins both Vitest and `@vitest/coverage-v8` to 5.1.1. The current
resolved runner and coverage provider are aligned. When changing either
package, run the corresponding coverage task and its normal test/build lane.

## RustSec exception detail

Main contains `rsa 0.9.10`, reported under `RUSTSEC-2023-0071`. The dependency
is transitive through `openidconnect` and OpenDAL/reqsign cloud backends used
for S3, GCS, Azure, and OSS storage. It is not used by the local-first core
path. Keep the finding visible in the read-only audit and remove this
exception when a patched release or supported replacement is available; do
not mark it resolved without rerunning the audit.

Cargo update candidate [#5119](https://github.com/ugoite/ugoite/pull/5119) at
HEAD `7496bf472f1b2d0f0dd3551fc4319f8daa7fa7b2` (base
`0c9c3f347c743ea592435f500960c79f34e75515`) also resolves
`rsa 0.10.0-rc.19` through `webauthn-rs 0.6.2` → `webauthn-rs-core 0.6.2` →
`crypto-glue 0.2.2`. The candidate `Cargo.lock` blob is
`573c4e151b43db2cca7723c308a8c819b0265cf7`; main's pre-update lockfile blob
is `6602514a509c7e82186f0ba6db3e27239c3bb020`. On 2026-11-25,
`cargo-audit 0.23.0` with advisory database commit
`10db6998b5d74ec438bcf0763125bd425a068a8f` reported 19 advisory records
against the main lockfile and 18 against the candidate. The candidate resolves
the `ctutils 0.4.2` and `url 2.5.8` findings; it still reports
`RUSTSEC-2023-0071` for `rsa 0.9.10` and also reports that advisory for
`rsa 0.10.0-rc.19`. This is an unresolved audit result, not a clean audit.

The inspected WebAuthn call path uses RSA public-key signature verification
through `RS256VerifyingKey`; no private-key signing or decryption call was
found in that path. This is a bounded call-site review, not proof that every
transitive caller is safe. Forcing stable `rsa 0.10.0` currently fails
resolution because `crypto-glue 0.2.2` requires `rsa ^0.10.0-rc.18` with a
`sha2` feature not provided by the stable release. Keep the RC finding visible
and revisit it with a compatible upstream `crypto-glue`/WebAuthn release.

With `cargo-audit 0.23.0`, `cargo audit --locked` is unsupported. Run
`cargo audit --json` from the workspace root; it reads `Cargo.lock`. Pair it
with `cargo metadata --locked --format-version 1` when verifying the full
locked workspace graph. Do not use `--no-deps` for this check because it skips
dependency resolution.

The current lockfile resolves `event-listener` to `5.4.2`; it is not a
separately maintained exception. Re-run the audit when the Rust dependency
graph changes.

> The dated run/candidate records below are historical snapshots. Use the current
> operational status above for the latest main and open-candidate state.

## Renovate cutover evidence (historical snapshots)

The writer cutover merged through the merge queue in PR
[#5094](https://github.com/ugoite/ugoite/pull/5094) at source SHA
`e7a3006bed006b5ad1976431930a725ef46ea0c4`. Dependency graph and Dependabot
alerts are maintained separately from Renovate update-PR generation. The
workflow verifies the App's Dependabot alerts API access; this repository
setting evidence does not expose the security-update PR toggle. That toggle is
managed separately from the Renovate workflow.

| Evidence | Source | Result and limit |
| --- | --- | --- |
| App-backed extract | [run 45161867473](https://github.com/ugoite/ugoite/actions/runs/45161867473), main `db7559e4f818aec11a513915ea9011d08b1eaba9` | Success; inventory covered 36 files, 643 dependencies, and all five Deno manifests. |
| App-backed lookup | [run 45164810651](https://github.com/ugoite/ugoite/actions/runs/45164810651), same main SHA | Success; candidate lookup is not CI evidence. Pre-job run 45161895265 was cancelled and replaced. |
| App-backed full dry-run | [run 45161915530](https://github.com/ugoite/ugoite/actions/runs/45161915530), same main SHA | Success; no repository writes. |
| First live writer run | [run 45191937781](https://github.com/ugoite/ugoite/actions/runs/45191937781), cutover SHA `e7a3006bed006b5ad1976431930a725ef46ea0c4` | Success; App alert access succeeded and Renovate created [Dependency Dashboard #5099](https://github.com/ugoite/ugoite/issues/5099). This pre-grouping run created no update PRs. |
| Daily grouped writer run | [run 45319646701](https://github.com/ugoite/ugoite/actions/runs/45319646701), source SHA `6d51975cfb42e660d06b04deb01a28ce816435c8` | Success; the intended App identity check and Dependabot alerts API check passed. Dashboard #5099 had zero routine updates awaiting approval and zero awaiting schedule. The run created grouped candidates #5115–#5119. |
| Post-hold strict validation | Renovate CLI 44.257.9, `renovate-config-validator --strict renovate.json`, source `f7dc67715d57a884debb19f26ef411b7237056b3` | Passed locally against the current Renovate configuration. |
| Post-hold full dry-run | Renovate CLI 44.257.9, `--dry-run=full`, source `f7dc67715d57a884debb19f26ef411b7237056b3` | Completed with exit code 0 and no repository writes. The local GitHub token could not read vulnerability alerts; the App-authenticated live run below separately verifies that API access. |
| Main CI baseline at that point | [run #45322845599](https://github.com/ugoite/ugoite/actions/runs/45322845599), main SHA `1eea649bc71a018e60c93a5b7f04314fa5d28dbe` | All planned CI lanes and `ci-required` passed on that historical main baseline. This does not replace candidate-specific CI. |
| Deno workspace candidate, initial head | [PR #5115](https://github.com/ugoite/ugoite/pull/5115), head `3a0ef8a4b7d2e6e18064747a2d44f00f06fad0e1`; [CI run #45323095331](https://github.com/ugoite/ugoite/actions/runs/45323095331) | This earlier head includes all five Deno manifests and root `deno.lock` and passed its required CI lanes and CodeQL. It was superseded by the refreshed candidate recorded below. |
| GitHub Actions candidate, earlier heads | [PR #5117](https://github.com/ugoite/ugoite/pull/5117); [first CI run #45314368028](https://github.com/ugoite/ugoite/actions/runs/45314368028); [pre-hold regenerated-head CI run #45331135864](https://github.com/ugoite/ugoite/actions/runs/45331135864); [post-policy Renovate run #45342695976](https://github.com/ugoite/ugoite/actions/runs/45342695976) | The first candidate selected `ubuntu-26.04`, which Playwright did not support; that runner target remains held. The later candidate retained the supported runner but changed Node 24 to 26 and failed `tools/workspace_test.ts`. Renovate 44.257.9 extracted this input as `depName: node` with `packageName: actions/node-versions`, so the initial package-name matcher did not apply. PR #5164 corrected it. The earlier Saved SQL case A07 mismatch did not reproduce in the same-lane comparison below and is not attributed to the Node update. |
| Cargo workspace candidate, earlier heads | [PR #5119](https://github.com/ugoite/ugoite/pull/5119); [CI run #45322126633](https://github.com/ugoite/ugoite/actions/runs/45322126633) | Earlier candidates first failed to regenerate `Cargo.lock` for `keyring` 4, then failed adapter compatibility checks for `rig-agent` 0.47.0, `sqlparser` 0.64.0, and direct `p256` 0.14.0. The corresponding holds are reflected in current configuration; refreshed-candidate evidence appears below. |
| Other grouped candidates | [Dev Containers #5116](https://github.com/ugoite/ugoite/pull/5116), [mise #5118](https://github.com/ugoite/ugoite/pull/5118) | Both candidates' required CI and CodeQL checks passed on their current heads; neither is approved or merged by this record. |
| Writer attribution follow-up | [#5120](https://github.com/ugoite/ugoite/issues/5120) | Earlier PR #5115 received a head update at 03:58 UTC to `3a0ef8a4`; the timeline attributes that force-push to `renovate[bot]` without a matching workflow run. Newer Docker PR #5166 is also authored by `app/renovate`, while its head commit was force-pushed by `ugoite-renovate[bot]` during the intended workflow run. The intended run then reported that the PR already existed. This writer overlap remains unresolved. |
| Earlier update candidates | PRs [#5112](https://github.com/ugoite/ugoite/pull/5112), [#5113](https://github.com/ugoite/ugoite/pull/5113), and [#5114](https://github.com/ugoite/ugoite/pull/5114) | These earlier candidates were closed after saving their update heads. Replacement PRs #5155–#5157 were created by `app/renovate`; #5155 was closed during a transfer attempt, after which the Docker branch resurfaced as #5166. #5156 and #5157 remain open. No failing dependency update was merged. |

### Historical daily grouping rerun evidence (2026-11-24)

The Node matcher correction merged in PR [#5164](https://github.com/ugoite/ugoite/pull/5164) at main SHA `ba50170e5a4b81677e756e7ff631480c3a2bbd70`. The pinned Renovate CLI 44.257.9 strict configuration validator passed on that source. Runs [#45346979191](https://github.com/ugoite/ugoite/actions/runs/45346979191), [#45348656866](https://github.com/ugoite/ugoite/actions/runs/45348656866), and [#45349952115](https://github.com/ugoite/ugoite/actions/runs/45349952115) completed successfully; each verified the intended App slug and Dependabot alerts API access. The live inventory in those runs covered 36 files and 620 dependencies across ten managers:

| Manager | Files | Dependencies |
| --- | ---: | ---: |
| Cargo | 13 | 191 |
| Deno | 5 | 191 |
| Dev Container | 1 | 5 |
| Docker Compose | 3 | 1 |
| Dockerfile | 1 | 9 |
| GitHub Actions | 9 | 217 |
| mise | 1 | 3 |
| npm | 1 | 1 |
| Rust toolchain | 1 | 1 |
| regex | 1 | 1 |

The refreshed candidate state below is tied to exact heads; a successful Renovate run alone is not a dependency-candidate CI result.

| Candidate | Source and current head | Verification status |
| --- | --- | --- |
| Deno [#5115](https://github.com/ugoite/ugoite/pull/5115) | Head `3610b09d18ef850550894d5eab313ed35c2c6048`; CI [#45349124671](https://github.com/ugoite/ugoite/actions/runs/45349124671); CodeQL [#45349124899](https://github.com/ugoite/ugoite/actions/runs/45349124899) | Candidate CI and CodeQL completed successfully. Changed manifests share root `deno.lock`; Web and artifact lanes use frozen dependency verification. This head retains Starlight outside the held versions. Saved SQL case A07 passed here and on baseline main SHA `ba50170` in [main CI #45346619515](https://github.com/ugoite/ugoite/actions/runs/45346619515); the earlier mismatch did not reproduce in this same-lane comparison. |
| GitHub Actions [#5117](https://github.com/ugoite/ugoite/pull/5117) | Head `244f4010679de0dff5bbf25fa03a04eb31cb34f2`; CI [#45348936509](https://github.com/ugoite/ugoite/actions/runs/45348936509); CodeQL [#45348936439](https://github.com/ugoite/ugoite/actions/runs/45348936439) | This regenerated head retains Node 24 and does not select Ubuntu 26.04. Required CI and CodeQL passed, including mobile and Composition Studio E2E. |
| Cargo [#5119](https://github.com/ugoite/ugoite/pull/5119) | Head `70ca29921a5eb2f8ca9339f14f8f09a71f63beb5`; CI [#45349053433](https://github.com/ugoite/ugoite/actions/runs/45349053433); CodeQL [#45349053166](https://github.com/ugoite/ugoite/actions/runs/45349053166) | Candidate contains `Cargo.lock`; CodeQL, Rust check, artifact build, E2E, and other executed lanes passed. Rust tests failed because `rmcp` 3.11.0 no longer leaves `storage unavailable` in the CLI error chain; `ci-required` correctly failed on that required test result. The same Rust-test lane passed on main. The `rmcp` hold merged in [PR #5168](https://github.com/ugoite/ugoite/pull/5168); the later regeneration follow-up was recorded in issue #5147 at the time of this snapshot. Current source/transport work is tracked in [#5243](https://github.com/ugoite/ugoite/issues/5243). |
| Docker [#5166](https://github.com/ugoite/ugoite/pull/5166) | Head `e4ddccbf0f57bac1153e4bc9f4d20516c5a7f51c`; Renovate run [#45349952115](https://github.com/ugoite/ugoite/actions/runs/45349952115); CI [#45350203317](https://github.com/ugoite/ugoite/actions/runs/45350203317) | All selected CI and CodeQL checks passed, but the repository issue-link gate failed. The head commit was force-pushed by `ugoite-renovate[bot]` while the PR is authored by `app/renovate`; the intended run reported that a PR already existed for the branch. This does not establish single-writer PR creation. |

After the grouped run, Dashboard #5099 had no routine proposals waiting for approval or a manager-specific schedule. It still listed an errored Docker retry and old branches #5156–#5157 in “PR Edited (Blocked)”; these must be accounted for before writer exclusivity is claimed. The transfer attempt closed #5155 and deleted its branch, but the `app/renovate` integration recreated Docker PR #5166 while the intended workflow updated the same branch. Disable the separate writer at the repository integration boundary before transferring the remaining legacy candidates. Issue #5120 tracks that cutover and candidate disposition.

### Historical post-hold live run (2026-11-24)

The `rmcp` minor hold merged through the merge queue in [PR #5168](https://github.com/ugoite/ugoite/pull/5168), producing main SHA `f7dc67715d57a884debb19f26ef411b7237056b3`. Intended writer run [#45357790780](https://github.com/ugoite/ugoite/actions/runs/45357790780) completed successfully on that source. The App slug and Dependabot alerts API checks passed; the run extracted 36 files and 620 dependencies across all ten managers. Dashboard #5099 has no routine Pending Approval or Awaiting Schedule sections.

| Candidate | Source and current head | Verification status |
| --- | --- | --- |
| Cargo [#5119](https://github.com/ugoite/ugoite/pull/5119) | Head `b75e7db1b6715ade02a4a9347ac88fec86c94c20`; CI [#45358262824](https://github.com/ugoite/ugoite/actions/runs/45358262824); CodeQL [#45358262904](https://github.com/ugoite/ugoite/actions/runs/45358262904) | The branch regenerated its manifests and shared `Cargo.lock`; the lockfile resolves `rmcp` to 3.7.0. Required CI, `ci-required`, and CodeQL passed, including Rust tests and all selected E2E lanes. No dependency candidate was approved or merged. |
| Deno [#5115](https://github.com/ugoite/ugoite/pull/5115) | Head `2a07567cd45c55968e211c573722a953f96ab87c`; CI [#45358347015](https://github.com/ugoite/ugoite/actions/runs/45358347015); CodeQL [#45358347022](https://github.com/ugoite/ugoite/actions/runs/45358347022) | The update includes the root `deno.json`, `deno.lock`, `docsite/deno.json`, `frontend/deno.json`, and `tools/deno.json`; `e2e/deno.json` had no eligible change. All required CI lanes and CodeQL passed. Renovate warned that it could not re-extract the updated Docsite and frontend manifests; the branch and lockfile were still updated. Track candidate verification and extraction follow-up in [#5100](https://github.com/ugoite/ugoite/issues/5100). |

The same run reported `A pull request already exists` for the Docker branch and the Dashboard retained the errored Docker retry. App-authored legacy PRs #5156, #5157, and #5166 remain open, while #5116 and #5118 still appear under “PR Edited (Blocked)”. The run confirms the intended writer can refresh the Deno and Cargo groups, but writer exclusivity and safe disposition of those existing candidates remain unresolved in [#5120](https://github.com/ugoite/ugoite/issues/5120). A dependency candidate is not complete until its exact head passes the applicable required CI and CodeQL checks.

The same run also logged non-fatal Cargo and Docker upgrade-collision diagnostics. These and the Deno re-extraction warnings remain visible follow-up items in [#5100](https://github.com/ugoite/ugoite/issues/5100); they are not counted as successful dependency-candidate verification.

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
cargo audit --json
cargo metadata --locked --format-version 1
deno audit
```
