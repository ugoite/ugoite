import pytest
from app.sandbox.python_sandbox import run_script, SandboxExecutionError


def test_simple_execution():
    code = "return 1 + 1;"
    result = run_script(code, lambda m, p, b: None)
    assert result == 2


def test_host_call():
    def handler(method, path, body):
        if method == "GET" and path == "/test":
            return {"ok": True}
        return {"ok": False}

    code = """
    const res = host.call("GET", "/test");
    return res.ok;
    """
    result = run_script(code, handler)
    assert result is True


def test_execution_error():
    code = "throw new Error('boom');"
    with pytest.raises(SandboxExecutionError) as exc:
        run_script(code, lambda m, p, b: None)
    assert "boom" in str(exc.value)


def test_infinite_loop_fuel():
    code = "while(true) {}"
    with pytest.raises(Exception) as exc:
        run_script(code, lambda m, p, b: None, fuel_limit=100000)
    # The error message from wasmtime usually contains "all fuel consumed"
    assert "fuel" in str(exc.value).lower()
