# Ugoite specification

This directory combines human guidance with machine-readable product records.

Start with [`index.md`](index.md).

## Sources of truth

- REST implementation and generated contract: `crates/ugoite-server/src/lib.rs` and `/openapi.json`.
- Portable remote-operation contract: `crates/ugoite-api-client`.
- Application behavior: `crates/ugoite-core`.
- Filesystem/object-storage behavior: `crates/ugoite-storage` plus core modules.
- Browser behavior: `frontend` (currently server-backed).
- Task/CI surface: root `mise.toml`, `deno.json`, and `.github/workflows/ci.yml`.

Machine-readable registries under `features/`, `requirements/`, `ui/`, and `docs/version/` must use existing source/test paths. Planned capability must be labeled planned rather than represented as implemented.
