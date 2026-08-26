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
- `test:*`: authoritative assertions that may reuse `build:*` outputs but always execute when called; focused frontend/docsite tasks remain useful during development;
- `test`: the canonical non-E2E suite, including Rust and tooling tests plus the frontend and docsite coverage gates;
- `test:rust`: the canonical Rust test interface; cargo-nextest runs unit, integration, binary, and library tests, followed by `cargo test --workspace --doc --locked` for doctests;
- `test:smoke`: the focused Rust smoke interface uses the same nextest-plus-doctest split before the frontend smoke task;
- `test:frontend:coverage` and `test:docsite:coverage`: V8 coverage assertions with
  package-owned hard thresholds for authored frontend/docsite source;
- `package:*`: staging under `target/artifacts/` only; packaging must fail if required build outputs are absent;
- `verify:*`: checks packaged outputs without rebuilding them;
- `ci`: formatting check, lint, architecture/OpenAPI/type checks, and `test`;
- `ci:artifacts`: build/package/verify, a focused docsite-navigation E2E lane, E2E smoke plus Form-owned Asset acceptance, and release metadata validation;
- `ci:merge`: `ci` plus `ci:artifacts`;
- `ci:release`: `ci:merge`, npm packaging/verification, and the full E2E suite.

Hosted CI schedules the `ci-rust-check`, `ci-rust-test`, `ci-web`, and `artifacts` lanes in parallel, then the `ci-required` aggregator preserves the required status-check context. The three quality lanes run only `mise run ci:lane:rust-check`, `mise run ci:lane:rust-test`, and `mise run ci:lane:web`; they do not duplicate repository validation commands in GitHub Actions. The artifact lane runs only `mise run ci:artifacts` and owns Playwright/BuildKit setup plus verified artifact upload.

Rust-compiling lanes restore the Rust registry/git dependency cache without caching `target/`; `ci-rust-check` is the sole Cargo dependency archive writer, while `ci-rust-test`, `ci-web`, and `artifacts` are restore-only. `ci-web` is the sole Deno archive writer; `artifacts` may restore it, but does not write it. sccache owns compiler artifact reuse in all Rust-compiling lanes: it is read-only for pull requests and merge queues and writes only on successful `main` pushes. Playwright browser and BuildKit caches remain separately keyed and are refreshed only after successful pushes to `main`. Successful `main` runs upload the verified artifact set using the logical names `ugoite-docsite-pages`, `ugoite-runtime-image`, `ugoite-cli-linux`, `ugoite-helm-chart`, and `ugoite-artifact-manifest`.

The hosted runtime image uses Dockerfile's `runtime-prebuilt` target. It copies the canonical frontend and Rust release outputs into the image instead of compiling them again inside Docker. E2E tasks require the already loaded `ugoite:e2e` image and never invoke an image build. The default Dockerfile target remains a portable source build for direct Docker and Compose use.

After publication, `Release Publish` runs both release quick-start verifiers against the exact prepared version and source SHA. The release also uploads `docker-compose.release.yaml`, its checksum, and a manifest containing the prepared source SHA and published image digest. The container verifier checks that manifest, downloads that published Compose definition, pulls the versioned GHCR image, and starts it with `--no-build`; it does not build or load another image for verification. The CLI verifier uses the published checksum-verified installer and retains the local Space create/list assertions.

Only after the artifact and quick-start gates succeed, `Release Publish` renders `docs/version/changelog/<channel>.yaml` into the GitHub Release body. The repository-owned section uses a versioned start/end marker so reruns replace the channel section and preserve GitHub's generated commit summary without duplication. Invalid, mismatched, or incomplete channel sources fail before the release body is changed.

The focused docsite-navigation lane is intentionally separate from smoke/full
runtime E2E. It builds the docsite through the canonical `build:docsite`
inputs, restores the versioned Playwright browser cache, keeps the explicit
browser-install step as a cache-miss fallback, previews the static artifact,
and verifies Starlight navigation semantics before the heavier runtime-backed
smoke suite runs.

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
