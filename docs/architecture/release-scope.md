# Release Scope

The pre-release Rust and Deno rearchitecture keeps the formal release scope
focused on the current production path:

- Rust `ugoite-server` for the browser HTTP, auth, static asset, and MCP adapter
  runtime.
- Shared Rust crates for domain, storage, core services, CLI, WASM wrappers, and
  release-critical repository tooling.
- Deno as the repository TypeScript runtime for frontend, docsite, E2E, and
  lightweight tooling.
- `ci-required` and `codeql-required` as the normal pull request checks, with
  heavier merge and release validation behind `mise run ci:merge` and
  `mise run ci:release`.

The release scope does not include browser-owned storage, OPFS or IndexedDB
persistence, peer-to-peer sync, relay protocols, distributed trust graphs, or a
signed operation log. Those belong to the North Star and future architecture
work.

The practical rule is simple: improve the current server-backed browser and
direct-core CLI paths, but keep server, CLI, frontend, WASM, and storage code on
thin adapter boundaries so future runtimes can be added without rewriting the
core model.
