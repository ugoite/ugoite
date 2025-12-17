"""Tests for the Python Sandbox."""

import pytest

from app.sandbox.python_sandbox import SandboxExecutionError, run_script


def test_simple_execution() -> None:
    """Test simple arithmetic execution."""
    code = "return 1 + 1;"
    result = run_script(code, lambda _m, _p, _b: None)
    assert result == 2  # noqa: PLR2004


def test_host_call() -> None:
    """Test host call functionality."""

    def handler(method: str, path: str, _body: dict | None) -> dict:
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
    """Test execution error handling."""
    code = "throw new Error('boom');"
    with pytest.raises(SandboxExecutionError) as exc:
        run_script(code, lambda _m, _p, _b: None)
    assert "boom" in str(exc.value)


def test_infinite_loop_fuel() -> None:
    """Test fuel limit for infinite loops."""
    code = "while(true) {}"
    with pytest.raises(Exception) as exc:  # noqa: PT011
        run_script(code, lambda _m, _p, _b: None, fuel_limit=100000)
    # The error message from wasmtime usually contains "all fuel consumed"
    assert "fuel" in str(exc.value).lower()
