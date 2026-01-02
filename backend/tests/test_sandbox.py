"""Tests for the WebAssembly sandbox."""

import importlib.resources
from pathlib import Path
from unittest.mock import patch

import pytest
from ieapp.sandbox import (
    SandboxError,
    SandboxExecutionError,
    run_script,
)


def _sandbox_available() -> bool:
    try:
        ref = importlib.resources.files("ieapp.sandbox") / "sandbox.wasm"
        with importlib.resources.as_file(ref) as wasm_path:
            return Path(wasm_path).exists()
    except (FileNotFoundError, OSError, RuntimeError):
        return False


pytestmark = pytest.mark.skipif(
    not _sandbox_available(),
    reason="sandbox.wasm missing; run build_sandbox.py to enable sandbox tests",
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


def test_missing_wasm_raises(tmp_path: Path) -> None:
    """If the Wasm artifact is missing, running scripts should raise SandboxError."""
    # We patch importlib.resources.as_file to yield a non-existent path
    with patch("importlib.resources.as_file") as mock_as_file:
        # Create a context manager mock
        mock_ctx = mock_as_file.return_value
        mock_ctx.__enter__.return_value = tmp_path / "missing.wasm"

        with pytest.raises(SandboxError):
            run_script("return 1;", _noop_handler)
