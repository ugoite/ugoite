"""Sandbox execution removed (wasmtime sandbox deprecated)."""

from __future__ import annotations

from typing import TYPE_CHECKING, NoReturn

if TYPE_CHECKING:
    from collections.abc import Callable


class SandboxError(RuntimeError):
    """Base exception for sandbox errors."""


class SandboxTimeoutError(SandboxError):
    """Raised when sandbox execution times out."""


class SandboxExecutionError(SandboxError):
    """Raised when sandbox execution fails or returns an error."""


def run_script(
    _code: str,
    _host_call_handler: Callable[[str, str, dict[str, object] | None], object],
    *,
    timeout_seconds: int = 30,
    fuel_limit: int = 1_000_000,
) -> NoReturn:
    """Raise because sandbox execution is no longer supported."""
    _ = timeout_seconds
    _ = fuel_limit
    msg = "Sandbox execution removed: Wasm sandbox is no longer part of ugoite."
    raise SandboxError(msg)
