"""Tests for the WebAssembly sandbox."""

from pathlib import Path

import pytest

from app.sandbox.python_sandbox import (
    SandboxError,
    SandboxExecutionError,
    run_script,
)


def _noop_handler(_method: str, _path: str, _body: dict | None) -> None:
    """No-op host call handler for tests."""


def test_simple_execution() -> None:
    """Test basic script execution returns correct result."""
    code = "return 1 + 1;"
    expected_result = 2
    result = run_script(code, _noop_handler)
    assert result == expected_result


def test_host_call() -> None:
    """Test host call functionality works correctly."""

    def handler(method: str, path: str, _body: dict | None) -> dict[str, bool]:
        if method == "GET" and path == "/test":
            return {"ok": True}
        return {"ok": False}

    code = """
    const res = host.call("GET", "/test");
    return res.ok;
    """
    result = run_script(code, handler)
    assert result is True


def test_execution_error() -> None:
    """Test that script errors are properly raised as SandboxExecutionError."""
    code = "throw new Error('boom');"
    with pytest.raises(SandboxExecutionError) as exc:
        run_script(code, _noop_handler)
    assert "boom" in str(exc.value)


def test_infinite_loop_fuel() -> None:
    """Test that infinite loops are stopped by fuel exhaustion."""
    code = "while(true) {}"
    fuel_limit = 100000
    with pytest.raises(Exception, match="fuel") as exc:
        run_script(code, _noop_handler, fuel_limit=fuel_limit)
    assert "fuel" in str(exc.value).lower()


def test_missing_wasm_raises(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    """If the Wasm artifact is missing, running scripts should raise SandboxError.

    This test avoids mutating the real repository artifact by monkeypatching
    `SANDBOX_WASM_PATH` to a non-existent path under `tmp_path` so tests are
    safe to run in parallel.
    """
    missing = tmp_path / "missing.wasm"
    monkeypatch.setattr(
        "app.sandbox.python_sandbox.SANDBOX_WASM_PATH", missing,
    )

    with pytest.raises(SandboxError):
        run_script("return 1;", _noop_handler)
