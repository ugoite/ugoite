---
title: 'CI and release gates'
---

The required build/test gate is `.github/workflows/ci.yml`. Separate workflows run CodeQL and validate required pull-request body sections/issue links.

| Event | Hosted CI behavior |
|---|---|
| pull request to `main` | validation, canonical build/package steps, a focused docsite-navigation E2E lane, artifact verification, and E2E smoke |
| merge queue to `main` | same merge gate as pull requests against the queued head |
| push to `main` | merge gate plus shared-cache refresh and verified artifact upload |

Root task composition:

- `build:*`: deterministic compile/build steps with declared inputs and outputs;
- `test:*`: authoritative assertions that may reuse `build:*` outputs but always execute when called;
- `package:*`: staging under `target/artifacts/` only; packaging must fail if required build outputs are absent;
- `verify:*`: checks packaged outputs without rebuilding them;
- `ci`: formatting check, lint, architecture/OpenAPI/type checks, and non-E2E tests;
- `ci:merge`: `ci`, build/package/verify, a focused docsite-navigation E2E lane, and E2E smoke;
- `ci:release`: `ci:merge` plus the full E2E suite.

Hosted CI restores Rust, Deno, and BuildKit caches on every event. Shared project caches are refreshed only after successful pushes to `main`. Successful `main` runs also upload the verified artifact set using the logical names `ugoite-docsite-pages`, `ugoite-runtime-image`, `ugoite-cli-linux`, `ugoite-helm-chart`, and `ugoite-artifact-manifest`.

The hosted runtime image uses Dockerfile's `runtime-prebuilt` target. It copies the canonical frontend and Rust release outputs into the image instead of compiling them again inside Docker. E2E tasks require the already loaded `ugoite:e2e` image and never invoke an image build. The default Dockerfile target remains a portable source build for direct Docker and Compose use.

The focused docsite-navigation lane is intentionally separate from smoke/full
runtime E2E. It builds the docsite through the canonical `build:docsite`
inputs, installs Playwright browsers explicitly for that lane, previews the
static artifact, and verifies Starlight navigation semantics before the heavier
runtime-backed smoke suite runs.

Deployable artifacts are staged below `target/artifacts/` with a machine-readable `manifest.json` and `SHA256SUMS`. This layout is for promotion by later workflows; deployment workflows must not compile source code again.

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
```

Build identity currently includes explicit environment such as `DOCSITE_ORIGIN`, `DOCSITE_BASE`, and `UGOITE_IMAGE_TAG`. To force a clean rebuild, remove `target/rust`, `target/wasm`, `target/artifacts`, `frontend/.output`, `docsite/dist`, and `frontend/src/lib/generated/ugoite_wasm.wasm`.

All task names are root `mise.toml` tasks; package-scoped task syntax is invalid.
