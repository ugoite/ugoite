---
title: 'CI and release gates'
---

The required build/test gate is `.github/workflows/ci.yml`. Separate workflows run CodeQL and validate required pull-request body sections/issue links.

| Event | Command |
|---|---|
| pull request to `main` | `mise run ci` |
| merge queue or push to `main` | `mise run ci:merge` |

Root task composition:

- `build:*`: deterministic compile/build steps with declared inputs and outputs;
- `test:*`: authoritative assertions that may reuse `build:*` outputs but always execute when called;
- `package:*`: staging under `target/artifacts/` only; packaging must fail if required build outputs are absent;
- `verify:*`: checks packaged outputs without rebuilding them;
- `ci`: formatting check, lint, architecture/OpenAPI/type checks, and non-E2E tests;
- `ci:merge`: `ci`, build/package/verify, and E2E smoke;
- `ci:release`: `ci:merge` plus the full E2E suite.

Deployable artifacts are staged below `target/artifacts/` with a machine-readable `manifest.json` and `SHA256SUMS`. This layout is for promotion by later workflows; deployment workflows must not compile source code again.

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
