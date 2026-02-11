#!/usr/bin/env python3
"""Sandbox build script removed (sandbox feature deprecated)."""


def build_sandbox() -> None:
    """Raise because sandbox artifacts are no longer supported."""
    msg = "Sandbox build removed: Wasm sandbox is no longer part of ugoite."
    raise RuntimeError(msg)


if __name__ == "__main__":
    msg = "Sandbox build removed: Wasm sandbox is no longer part of ugoite."
    raise SystemExit(msg)
