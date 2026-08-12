---
title: 'CI and release gates'
---

The required build/test gate is `.github/workflows/ci.yml`. Separate workflows run CodeQL, validate required pull-request body sections/issue links, promote docsite Pages, and publish versioned non-docsite release artifacts.

| Event | Hosted CI behavior |
|---|---|
| pull request to `main` | validation, canonical build/package steps, a focused docsite-navigation E2E lane, artifact verification, and E2E smoke |
| merge queue to `main` | same merge gate as pull requests against the queued head |
| push to `main` | merge gate plus shared-cache refresh and verified artifact upload |
| merge of the Release Please PR to `main` | Release Please creates `v<version>` and the `Release Publish` workflow validates/publishes the non-docsite release artifacts |

Root task composition:

- `build:*`: deterministic compile/build steps with declared inputs and outputs;
- `test:*`: authoritative assertions that may reuse `build:*` outputs but always execute when called;
- `test:frontend:coverage` and `test:docsite:coverage`: V8 coverage assertions with
  package-owned hard thresholds for authored frontend/docsite source;
- `package:*`: staging under `target/artifacts/` only; packaging must fail if required build outputs are absent;
- `verify:*`: checks packaged outputs without rebuilding them;
- `ci`: formatting check, lint, architecture/OpenAPI/type checks, and non-E2E tests;
- `ci:merge`: `ci`, build/package/verify, a focused docsite-navigation E2E lane, and E2E smoke;
- `ci:release`: `validate:release`, `ci:merge`, npm packaging/verification, and the full E2E suite.

Hosted CI restores Rust, Deno, Playwright browser, and BuildKit caches on every event. Shared project caches are refreshed only after successful pushes to `main`. Release build jobs use the same Rust/Deno dependency cache policy, with one Ubuntu release path writing the shared Deno key to avoid concurrent partial saves, and a separate release-image BuildKit scope. Successful `main` runs also upload the verified artifact set using the logical names `ugoite-docsite-pages`, `ugoite-runtime-image`, `ugoite-cli-linux`, `ugoite-helm-chart`, and `ugoite-artifact-manifest`.

The hosted runtime image uses Dockerfile's `runtime-prebuilt` target. It copies the canonical frontend and Rust release outputs into the image instead of compiling them again inside Docker. E2E tasks require the already loaded `ugoite:e2e` image and never invoke an image build. The default Dockerfile target remains a portable source build for direct Docker and Compose use.

After publication, `Release Publish` runs both release quick-start verifiers against the exact prepared version and source SHA. The release also uploads `docker-compose.release.yaml`, its checksum, and a manifest containing the prepared source SHA and published image digest. The container verifier checks that manifest, downloads that published Compose definition, pulls the versioned GHCR image, and starts it with `--no-build`; it does not build or load another image for verification. The CLI verifier uses the published checksum-verified installer and retains the local Space create/list assertions.

The focused docsite-navigation lane is intentionally separate from smoke/full
runtime E2E. It builds the docsite through the canonical `build:docsite`
inputs, restores the versioned Playwright browser cache, keeps the explicit
browser-install step as a cache-miss fallback, previews the static artifact,
and verifies Starlight navigation semantics before the heavier runtime-backed
smoke suite runs.

The required `ci-required` job runs both coverage tasks on pull requests,
merge queues, and pushes to `main`. Their package-level Vitest thresholds are
hard merge gates: a coverage regression fails the required job. The active
`main only pr` repository ruleset must require the `ci-required` status-check
context; a successful push-to-`main` run alone is not merge enforcement.
Frontend's unit coverage gate explicitly covers the portable Rust/WASM protocol
boundary in `frontend/src/lib/ugoite-client/protocol.ts`; UI behavior remains
covered by behavior tests and E2E. Docsite coverage includes authored
`src/**/*.{js,mjs,ts,tsx}` while excluding test files, `src/env.d.ts`, and
Astro's framework-only `src/content.config.ts`.

## Release promotion contract

`Release Publish` has two deliberately separate phases:

1. A push to `main` runs Release Please, which creates or updates the release
   planning PR. It does not publish artifacts.
2. Merging that planning PR creates the tag and GitHub Release. The same
   workflow validates the tagged source, builds the release artifact set from
   that exact tagged SHA, verifies it, and publishes it. It does not publish on
   every `main` push. The `workflow_dispatch` input for an existing validated
   `v<SemVer>` tag is retained only for explicit republish/recovery.

The repository-level Actions setting is part of this contract. The
organization policy must first allow repositories to enable PR creation; an
organization administrator must lift the current organization-level block.
Then, in the repository settings under **Actions → General → Workflow
permissions**, keep the default at **Read repository contents permission** and
enable **Allow GitHub Actions to create and approve pull requests**. The
workflow keeps top-level `permissions: {}` and grants `contents: write` and
`issues: write` and `pull-requests: write` to the `release-please` job so it
can apply its planning-PR labels; the separate
`build-cli` and `build-npm-helm` jobs each grant `id-token: write` to attest
their artifacts, plus `attestations: write` for the attestation API;
`publish-image` grants `contents: read`, `packages: write`, and
`id-token: write` for tagged-source checkout, image publication, and its
provenance; and `publish-release-assets` grants
`contents: write` and `packages: write` to publish the already-validated
artifacts. The remaining jobs use read-only permissions. No job outside
`release-please` receives `issues: write` or `pull-requests: write`, and no
ordinary workflow is made write-capable. These are the minimum job-scoped
permissions for Release Please labels, artifact attestations, and publication.

The release artifact jobs attest the CLI archive, npm installer, and Helm chart
archive with `actions/attest-build-provenance`; the image job keeps BuildKit
`provenance: true`. Hosted release evidence must verify those attestations and
the image provenance against the exact release tag/source SHA and record the
published image and chart digests before the release is considered complete.

The first stable pre-v1 release is a one-time bootstrap. The config's
`release-as: 0.1.0` pins the candidate to the checked-in version, and its
`bootstrap-sha` is the current `main` boundary so the old non-Conventional
Commit history is not replayed. After the `v0.1.0` Release Please PR merges,
remove both temporary config keys and update the focused release contract test.
From then on, normal Conventional Commit versioning is authoritative.

If a failed planner run leaves `release-please--branches--main` without an
open Release Please PR, verify that no release PR exists, delete only that
orphaned planner ref, and rerun the `Release Publish` workflow on `main`. Do
not delete tags or published releases as part of planner recovery.

Deployable artifacts are staged below `target/artifacts/` with a machine-readable `manifest.json` and `SHA256SUMS`. The release workflow also emits installer-compatible CLI archives named `ugoite-v<version>-<target>.tar.gz` plus per-file checksums, publishes `@ugoite/ugoite` to GitHub Packages, publishes `ghcr.io/ugoite/ugoite:<version>`, and pushes the Helm chart to `oci://ghcr.io/ugoite/charts`.

`.github/workflows/docsite-pages.yml` promotes the verified `ugoite-docsite-pages` artifact from a successful push-to-`main` CI run. It validates the upstream run identity, manifest, checksums, and current Pages origin/base metadata, then deploys the downloaded static files without checking out or rebuilding source.

Build reuse and test-result caching are different concepts. `sources`/`outputs` may skip deterministic `build:*` work when inputs are unchanged, but they are never evidence that a test passed.

Current artifact layout:

```text
target/artifacts/
  manifest.json
  SHA256SUMS
  docsite/
  cli/
  helm/
  image/
  npm/
```

Build identity currently includes explicit environment such as `DOCSITE_ORIGIN`, `DOCSITE_BASE`, and `UGOITE_IMAGE_TAG`. To force a clean rebuild, remove `target/rust`, `target/wasm`, `target/artifacts`, `frontend/.output`, `docsite/dist`, and `frontend/src/lib/generated/ugoite_wasm.wasm`.

All task names are root `mise.toml` tasks; package-scoped task syntax is invalid.
