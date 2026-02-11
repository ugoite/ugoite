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
- Core operations are contained in `ugoite-core` (Rust).
- Multiple language bindings are provided:
  - Python bindings for backend and `ugoite-cli`
  - WebAssembly bindings for browser contexts (future target)

## Data Portability

The data model is designed to remain:
- human-readable (JSON + Markdown)
- easy to back up
- storage-provider agnostic

