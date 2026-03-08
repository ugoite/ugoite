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
  adapter used by the current backend and CLI stack.
- Multiple language bindings are provided:
  - Python bindings from `ugoite-core` for backend and `ugoite-cli`
  - WebAssembly bindings can target the portable `ugoite-minimum` layer in
    future browser contexts

## Data Portability

The data model is designed to remain:
- human-readable (JSON + Markdown)
- easy to back up
- storage-provider agnostic
