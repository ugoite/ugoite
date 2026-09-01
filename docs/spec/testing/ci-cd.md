---
title: 'CI and release gates'
---

The required build/test gate is `.github/workflows/ci.yml`. Separate workflows run CodeQL, validate required pull-request body sections/issue links, promote docsite Pages, and publish versioned non-docsite release artifacts.

| Event | Hosted CI behavior |
|---|---|
| pull request to `main` | validation, canonical build/package steps, a focused docsite-navigation E2E lane, artifact verification, and E2E smoke |
| merge queue to `main` | same merge gate as pull requests against the queued head |
| push to `main` | merge gate plus shared-cache refresh and verified artifact upload |
| manual `Release Candidate` dispatch | builds and verifies the exact selected source SHA and stores a candidate bundle |
| manual `Release Publish` dispatch | verifies an operator-selected candidate bundle and promotes its exact artifacts without rebuilding |

Root task composition:

- `build:*`: deterministic compile/build steps with declared inputs and outputs;
- `test:*`: authoritative assertions that may reuse `build:*` outputs but always execute when called; focused frontend/docsite tasks remain useful during development;
- `test`: the canonical non-E2E suite, including Rust and tooling tests plus the frontend and docsite coverage gates;
- `test:rust`: the canonical Rust test interface; cargo-nextest runs unit, integration, binary, and library tests, followed by `cargo test --workspace --doc --locked` for doctests;
- `test:smoke`: the focused Rust smoke interface uses the same nextest-plus-doctest split before the frontend smoke task;
- `test:frontend:coverage` and `test:docsite:coverage`: V8 coverage assertions with
  package-owned hard thresholds for authored frontend/docsite source;
- `package:*`: staging under `target/artifacts/` only; packaging must fail if required build outputs are absent;
- `verify:*`: checks packaged outputs without rebuilding them;
- `ci`: formatting check, lint, architecture/OpenAPI/type checks, and `test`;
- `ci:artifacts`: build/package/verify, a focused docsite-navigation E2E lane, E2E smoke plus Form-owned Asset acceptance, and version validation;
- `ci:merge`: `ci` plus `ci:artifacts`;
- `ci:release`: `ci:merge`, npm packaging/verification, and the full E2E suite.

Hosted CI schedules the `ci-rust-check`, `ci-rust-test`, `ci-web`, and `artifacts` lanes in parallel, then the `ci-required` aggregator preserves the required status-check context. The three quality lanes run only `mise run ci:lane:rust-check`, `mise run ci:lane:rust-test`, and `mise run ci:lane:web`; they do not duplicate repository validation commands in GitHub Actions. The artifact lane runs only `mise run ci:artifacts` and owns Playwright/BuildKit setup plus verified artifact upload.

The required Rust suite covers the memory and filesystem implementations. The
optional `crates/ugoite-storage/tests/s3_contract.rs` integration test runs only
when both `UGOITE_S3_TEST_ENDPOINT` and `UGOITE_S3_TEST_BUCKET` are configured;
it invokes `OpendalPublicationStore::verify_contract` against that explicitly
selected deployment backend. This test is intended for manual or release
validation and is not part of required CI. Runtime startup continues to verify
the backend selected by the deployment before shared publication is admitted.

Rust-compiling lanes restore the Rust registry/git dependency cache without caching `target/`; `ci-rust-check` is the sole Cargo dependency archive writer, while `ci-rust-test`, `ci-web`, and `artifacts` are restore-only. `ci-web` is the sole Deno archive writer; `artifacts` may restore it, but does not write it. sccache owns compiler artifact reuse in all Rust-compiling lanes: it is read-only for pull requests and merge queues and writes only on successful `main` pushes. Playwright browser and BuildKit caches remain separately keyed and are refreshed only after successful pushes to `main`. Successful `main` runs upload the verified artifact set using the logical names `ugoite-docsite-pages`, `ugoite-runtime-image`, `ugoite-cli-linux`, `ugoite-helm-chart`, and `ugoite-artifact-manifest`.

The hosted runtime image uses Dockerfile's `runtime-prebuilt` target. It copies the canonical frontend and Rust release outputs into the image instead of compiling them again inside Docker. E2E tasks require the already loaded `ugoite:e2e` image and never invoke an image build. The default Dockerfile target remains a portable source build for direct Docker and Compose use.

## Release contract

`version.txt` is the only prepared-version authority. `version:sync` updates
Cargo, npm, Helm, and `Cargo.lock` projections; `version:check` verifies them.
Stable `v<version>` tags are the published-version ledger. Historical alpha and
beta tags are excluded from stable-version calculation.

The first stable release uses the already prepared `0.1.0` as-is. Later
pre-1.0 releases use `release:prepare compatible|breaking`, which compares the
prepared version with the latest stable tag before updating projections. A
compatible change advances the patch; a breaking change advances the minor.
Preparation never creates a tag, release, or registry artifact.

`Release Candidate` checks out one exact source SHA, runs the canonical
release-grade build/package/verification lanes, and stores the resulting
artifact set plus a schema-versioned `candidate-manifest.json`. The manifest
records version, source SHA, CI run identity, verification state, artifact
digests, and platform information. Its candidate ID is the SHA-256 of the
exact manifest bytes; the manifest does not contain that ID.

`Release Publish` accepts a candidate run and candidate ID, downloads the
candidate artifact, and invokes `release:verify-candidate` before promotion.
The promotion job contains no compile, build, pack, package, or repackage step.
It publishes the exact CLI archives, npm tarball, Helm archive, release Compose
assets, and container digest from the manifest. Missing identities are
published, matching identities are verified and skipped, and mismatches abort.
Immutable versioned identities are verified before the GitHub Release is
finalized; mutable aliases are updated by a separate final job after
quick-start checks.

Candidate verification and publication verification are separate. The former
checks staged bytes; the latter downloads published assets and runs the public
CLI and Compose quick starts against those assets. Both workflows keep a
top-level `permissions: {}` boundary and grant only job-scoped permissions.

Detailed provenance evidence and planner-ref recovery remain follow-up work,
not additional v0.1 release authorities.

The focused docsite-navigation lane is intentionally separate from smoke/full
runtime E2E. It builds the docsite through the canonical `build:docsite`
inputs, restores the versioned Playwright browser cache, keeps the explicit
browser-install step as a cache-miss fallback, previews the static artifact,
and verifies Starlight navigation semantics before the heavier runtime-backed
smoke suite runs.

The canonical `check:mitase` task consumes the published Mitase `v0.1.0`
release artifact for the host target. It verifies the pinned candidate
manifest digest and the selected target archive digest before extracting the
binary, and records the Mitase source SHA and candidate identity in the check
output. The default path does not build Mitase from Git; `MITASE_BIN` remains
available as an explicit local development override.

The required `ci-required` aggregator runs after all four lanes on pull
requests, merge queues, and pushes to `main`. It fails unless
`ci-rust-check`, `ci-rust-test`, `ci-web`, and `artifacts` are all successful.
The canonical `test` Mise task invokes both package-level Vitest coverage gates,
and `ci:lane:web` packs those same coverage tasks for Hosted CI, so their hard
thresholds remain merge gates without duplicating individual coverage commands
in GitHub Actions. The active `main only pr` repository ruleset must require
the `ci-required` status-check context; a successful push-to-`main` run alone
is not merge enforcement.

The root Mise graph is the repository quality contract: `ci` composes
`fmt:check`, `lint`, `check`, and `test`; `lint` composes Rust and Deno lint
tasks; `check` composes Rust, Deno, and repository contract checks. Hosted lane
tasks are packing adapters only and are covered by
`tools/coverage_gates_test.ts`, which explicitly asserts their semantic
composition and workflow entrypoints. `mise run ci` and `mise run ci:merge`
remain the developer-facing canonical interfaces.
Frontend's unit coverage gate explicitly covers the portable Rust/WASM protocol
boundary in `frontend/src/lib/ugoite-client/protocol.ts`; UI behavior remains
covered by behavior tests and E2E. Docsite coverage includes authored
`src/**/*.{js,mjs,ts,tsx}` while excluding test files, `src/env.d.ts`, and
Astro's framework-only `src/content.config.ts`.

Deployable artifacts are staged below `target/artifacts/` with a machine-readable
`manifest.json` and `SHA256SUMS`. Candidate runs additionally publish an exact
`candidate-manifest.json` and candidate bundle. The release workflow emits
installer-compatible CLI archives named `ugoite-v<version>-<target>.tar.gz`
plus per-file checksums and the release Compose assets, publishes
`@ugoite/ugoite` to GitHub Packages, publishes
`ghcr.io/ugoite/ugoite:<version>`, and pushes the Helm chart to
`oci://ghcr.io/ugoite/charts`.

`.github/workflows/docsite-pages.yml` promotes the verified `ugoite-docsite-pages` artifact from a successful push-to-`main` CI run. It validates the upstream run identity, manifest, checksums, and current Pages origin/base metadata, then deploys the downloaded static files without checking out or rebuilding source.

Build reuse and test-result caching are different concepts. `sources`/`outputs` may skip deterministic `build:*` work when inputs are unchanged, but they are never evidence that a test passed.

Current artifact layout:

```text
target/artifacts/
  manifest.json
  SHA256SUMS
  candidate-manifest.json  # candidate bundle only
  docker-compose.release.yaml(.sha256)  # candidate/release assets
  docsite/
  cli/
  helm/
  image/
  npm/
```

Build identity currently includes explicit environment such as `DOCSITE_ORIGIN`, `DOCSITE_BASE`, and `UGOITE_IMAGE_TAG`. To force a clean rebuild, remove `target/rust`, `target/wasm`, `target/artifacts`, `frontend/.output`, `docsite/dist`, and `frontend/src/lib/generated/ugoite_wasm.wasm`.

All task names are root `mise.toml` tasks; package-scoped task syntax is invalid.
