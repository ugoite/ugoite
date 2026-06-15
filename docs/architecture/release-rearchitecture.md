# Pre-release Rust and Deno rearchitecture

## Decision

Ugoite's release architecture converges on Rust for application, storage,
server, CLI, WASM, and release-critical tooling, and on TypeScript executed by
Deno for the frontend, docsite, E2E, and lightweight repository tooling.
Production runs the Rust server and browser JavaScript only.

Python, uv, Bun, npm, Node version pinning, Biome, mandatory Git hooks, and
subdirectory `mise.toml` files are transitional and must not appear in the
formal release architecture. Backward compatibility is not required before the
first formal release; migrations should remove superseded code and docs.

## Target layout

The Rust workspace will be split into domain, storage, core, server, CLI, WASM,
and xtask crates. The pure domain crate must always compile for
`wasm32-unknown-unknown`. Deno owns the repository-wide TypeScript workspace,
tasks, formatting, linting, and the single `deno.lock`.

Root `mise.toml` is the only contributor entry point and pins only Rust and
Deno. Cargo owns Rust checks; Deno owns TypeScript checks; GitHub Actions calls
the standard root tasks.

## Canonical commands

```bash
mise install
mise run setup
mise run dev
mise run fmt
mise run lint
mise run check
mise run test
mise run ci
mise run ci:merge
mise run ci:release
```

The normal pull request gate is `mise run ci`, the merge/main gate is
`mise run ci:merge`, and the release artifact gate is `mise run ci:release`.
Required status checks converge on `ci-required` and `codeql-required`.

## Migration phases

Phase 0 records this target and establishes the canonical commands. Phase 1
adopts the root Rust/Deno toolchain, Deno workspace, and single lockfile while
removing root npm tooling, sub-mise files, Biome configuration, Husky, and
pre-commit.

Later phases move crates into their target layout, replace FastAPI with a Rust
server, remove Python completely, finish the package-manager-free Deno
workspace, consolidate Actions, and harden release artifacts. Until those phases
land, Deno tasks may adapt existing package metadata and the development server
may still launch the transitional Python backend.
