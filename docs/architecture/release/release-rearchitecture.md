---
title: "Release rearchitecture status"
sidebar:
  order: 3
---

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
- production Passkey enrollment/login and optional invited OIDC;
- sponsored Agent Principals and append-only authorization audit history;
- channel-specific release communication and support rollout after a published
  version.

These items are future scope and must remain labeled as such.
