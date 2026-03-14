# ugoite-minimum

`ugoite-minimum` is the portable Rust layer that stays small enough for
`wasm32-unknown-unknown` builds and other lightweight consumers.

## Responsibility boundary

`ugoite-minimum` owns portable building blocks:

- pure data models and serde-friendly types
- integrity primitives such as checksums and HMAC helpers
- storage traits and URI normalization
- metadata reservation rules
- text helpers such as `compute_word_count`

`ugoite-core` owns the heavier adapter layer:

- OpenDAL-backed storage implementations
- entry, space, asset, and form workflows
- indexing, SQL execution, and other storage-aware orchestration
- Python bindings for the backend

The rule of thumb is simple: if a function is pure, portable, and does not need
OpenDAL, Iceberg, Parquet, or Python integration, it is a candidate for
`ugoite-minimum`. If it depends on storage adapters, indexing engines, or FFI,
it stays in `ugoite-core`.

## WASM posture

The crate is validated in CI and pre-commit with:

```bash
cargo test
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --lib
```

From the repo root, run the same build through:

```bash
mise run //ugoite-minimum:build:wasm
```

If you are already inside `ugoite-minimum/`, `mise run build:wasm` runs the
same task.

## Why only small extractions move here

`compute_word_count` now lives in `ugoite-minimum` because it is a portable text
primitive used by indexing but does not depend on any storage or runtime layer.
More complex property casting still lives in `ugoite-core` today because that
logic is coupled to the current form-definition JSON shape and warning
structures; we should only move it once there is a smaller portable API that
does not drag heavier indexing concerns into the minimum crate.
