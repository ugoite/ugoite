# Future-Proofing (Experimental)

This document captures forward-looking architecture ideas. These are not all
implemented today, but they are part of the intended direction.

## BYOAI (Bring Your Own AI)

Goal: users should be able to choose *their* AI provider/runtime without
rewriting the app.

Approach:
- Use **MCP** as the stable integration surface.
- Keep the backend as a protocol host + policy enforcement layer.
- Keep data access behind the same REST surface used by the UI.

Non-goals:
- Shipping a single bundled cloud AI provider as a hard dependency.

## Multi-Platform Core

To enable cross-platform operation and varied deployment targets:
- Portable domain logic and storage traits live in `ugoite-minimum` (Rust).
- `ugoite-core` depends on `ugoite-minimum` and provides the OpenDAL-backed
  adapter used by the current backend and native CLI stack.
- Multiple language interfaces are provided:
  - Python bindings from `ugoite-core` for backend integration
  - Native Rust binary for `ugoite-cli`
  - WebAssembly bindings can target the portable `ugoite-minimum` layer in
    future browser contexts

### Current responsibility split

| Layer | Keep here |
| --- | --- |
| `ugoite-minimum` | Portable models, storage traits, integrity helpers, metadata reservation rules, URI normalization, and pure text helpers such as `compute_word_count` |
| `ugoite-core` | OpenDAL-backed storage adapters, indexing orchestration, SQL/materialized views, CRUD flows, and Python bindings |

Portable helpers should only move into `ugoite-minimum` when they stay free of
OpenDAL, Iceberg, Parquet, Python bindings, and storage-aware orchestration.
That is why `compute_word_count` can live in the portable layer today, while the
larger property-casting pipeline in `ugoite-core::index` still stays in the
heavier core until it has a smaller transport-agnostic API.

## Data Portability

The data model is designed to remain:
- human-readable (JSON + Markdown)
- easy to back up
- storage-provider agnostic
