---
title: 'CI and release gates'
---

The required build/test gate is `.github/workflows/ci.yml`. Separate workflows run CodeQL and validate required pull-request body sections/issue links.

| Event | Command |
|---|---|
| pull request to `main` | `mise run ci` |
| merge queue or push to `main` | `mise run ci:merge` |

Root task composition:

- `ci`: formatting check, lint, architecture/OpenAPI/type checks, and tests;
- `ci:merge`: `ci`, frontend/docsite builds, and E2E smoke;
- `ci:release`: local release candidate gate adding release Rust builds, WASM check, and local image verification.

The uploaded repository does not contain a publishing workflow, so `ci:release` proves local readiness only. All task names are root `mise.toml` tasks; package-scoped task syntax is invalid.
