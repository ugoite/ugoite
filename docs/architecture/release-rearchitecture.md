# Release rearchitecture status

The repository has completed the current Rust-centered consolidation:

- one Cargo workspace with domain, storage, core, server, CLI, WASM, portable
  API client, and xtask crates;
- one Deno workspace for frontend, docsite, tools, and end-to-end tests;
- one runtime container containing the server, CLI, and static browser files;
- one root `mise.toml` command surface;
- generated OpenAPI and architecture checks in CI.

The remaining architectural work is product capability, not stack migration:

- browser-local Space persistence;
- optional synchronization/relay semantics;
- production passkey/TOTP enrollment and login;
- managed service accounts and audit-log APIs;
- release automation beyond the local `ci:release` gate.

These items are future scope and must remain labeled as such.
