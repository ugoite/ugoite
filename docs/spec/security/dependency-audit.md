---
title: Dependency audit exceptions
---

The current locked dependency graph has one unresolved RustSec advisory for
`rsa 0.9.10` (RUSTSEC-2023-0071). No patched upstream release is available for
the advisory at this version boundary.

The dependency is transitive through two separately scoped surfaces:

- `openidconnect`, used by the server's supported OIDC protocol adapter;
- OpenDAL/reqsign cloud backends, used by S3/GCS/Azure/OSS storage support.

The package is not used by the local-first core data path. Removing it would
require dropping those optional transport/authentication surfaces, so this
release keeps the locked graph and records the exception rather than silently
ignoring the audit result. `event-listener` is pinned through the lockfile at
5.4.2, which removes its fixable advisory.

Operators should rerun `cargo audit --locked --json` when the dependency graph
or release channel changes. The exception must be removed as soon as an
upstream patched `rsa` release or a supported replacement is available.
