---
title: Dependency audit exceptions
---

This page records only active dependency exceptions, their reasons, and the
conditions for revisiting them. It is not a version or run-status authority:
manifests, lockfiles, toolchain files, and workflow configuration own current
versions and policy; candidate results belong to their pull requests and Actions
checks.

## Active compatibility holds

`renovate.json` is the authority for the exact held ranges and matching rules.
Routine vulnerability fixes remain eligible under the separate security-update
rule unless an explicit supported limitation is recorded below.

| Area                                                | Reason for the hold                                                                                                                                                          | Revisit when                                                                                                                                                          |
| --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Arrow / Parquet / Iceberg / DataFusion / OpenDAL    | These crates share concrete Arrow types; an isolated major or minor update can break the integrated stack.                                                                   | Design a coordinated update and validate the crates, Iceberg adapters, SQL, and storage together.                                                                     |
| `keyring` major updates                             | The v4 line changes the application API and backend-store model; the CLI currently requests a feature that is not provided there.                                            | Migrate the CLI to a supported backend and verify credential read/write behavior. Follow-up: [#5121](https://github.com/ugoite/ugoite/issues/5121).                   |
| `rig-agent` minor and major updates                 | The adapter relies on API behavior that changed in a newer candidate and lacks migration coverage.                                                                           | Migrate the adapter with provider, tool-call, and result behavior tests. Follow-up: [#5242](https://github.com/ugoite/ugoite/issues/5242).                            |
| `rmcp` minor updates                                | The newer public transport error does not retain the response body; retrying a mutating request could repeat an operation, and dropping the assertion would weaken coverage. | Preserve useful HTTP diagnostics without retrying mutations, then verify the focused Rust behavior. Follow-up: [#5243](https://github.com/ugoite/ugoite/issues/5243). |
| GitHub Actions Node major updates                   | The workflow runtime must remain aligned with the repository's CI toolchain contract.                                                                                        | Change the toolchain authority and workflow together, then validate the affected CI lanes.                                                                            |
| `iceberg` and `iceberg-datafusion` Git dependencies | These dependencies are selected by a reviewed upstream revision rather than a registry version.                                                                              | Review a compatible revision and verify saved-Space reopen, bounded queries, storage, and API behavior.                                                               |
| Mitase bootstrap artifacts                          | The downloaded bootstrap tools are part of the specification-validation boundary and must match their checked digests.                                                       | Update the tool version and all associated digests together; run bootstrap and Mitase contract checks.                                                                |
| MinIO CI image                                      | The S3 authorization scenario depends on the image's specific behavior.                                                                                                      | Review the image change and rerun the S3/ACL scenario and affected CI lane.                                                                                           |
| `sccache` action                                    | The pinned action controls CI cache setup and behavior.                                                                                                                      | Update the action pin and verify cache setup and affected CI lanes.                                                                                                   |
| TypeScript major updates                            | A repository tool uses compiler APIs removed by the next major.                                                                                                              | Update that consumer and validate frontend changed-file type checking.                                                                                                |
| Starlight                                           | A newer candidate failed Docsite static rendering through Astro middleware resolution.                                                                                       | Revisit a candidate after the runtime resolution issue is addressed; validate Docsite build and navigation.                                                           |
| Playwright                                          | A newer candidate caused the Docsite navigation suite to load as CommonJS, where its `import.meta` use failed.                                                               | Revisit a candidate that preserves module loading; validate Docsite navigation and browser setup.                                                                     |
| Ubuntu Actions runner                               | The newer runner image is not supported by the pinned Playwright Chromium tooling.                                                                                           | Revisit after browser tooling supports the image; validate browser installation, Docsite navigation, and affected E2E lanes.                                          |
| SolidStart                                          | A newer major removes `defineConfig`, which the frontend build configuration uses.                                                                                           | Revisit after the frontend configuration is migrated; run frontend type checking, tests, and build.                                                                   |
| SolidJS runtime and router major updates            | The newer major changes exports and runtime behavior used by the frontend.                                                                                                   | Revisit after the frontend supports the new exports and behavior; run the Web type check, tests, and build.                                                           |

## Unresolved RustSec exception

The current Rust dependency graph is reported for `RUSTSEC-2023-0071` (`rsa`).
The dependency enters through OIDC and cloud-storage signing paths; the alert is
not silently waived or hidden by the routine compatibility holds. Run
`cargo audit --json` and keep the finding visible until a patched, supported
graph is available. The broader Dependabot alert inventory is tracked in
[#4997](https://github.com/ugoite/ugoite/issues/4997).
