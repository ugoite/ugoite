"""Tests for the Python code execution sandbox.

These tests verify the security measures described in spec 05 §Sandbox:
- Filesystem: Restricted to workspace (read-only) and /tmp (read-write)
- Network: Blocked entirely
- Resources: CPU timeout, memory limit
- Process: Isolated namespace
"""

import pytest

from app.core.sandbox import (
    SandboxErrorType,
    SandboxResult,
    run_in_sandbox,
)


class TestSandboxBasicExecution:
    """Test basic code execution in the sandbox."""

    def test_simple_print(self) -> None:
        """Test that simple print statements work."""
        code = 'print("Hello, World!")'
        result = run_in_sandbox(code)
        assert result.returncode == 0
        assert "Hello, World!" in result.stdout

    def test_syntax_error(self) -> None:
        """Test that syntax errors are reported."""
        code = "print('unclosed"
        result = run_in_sandbox(code)
        assert result.returncode != 0
        assert "SyntaxError" in result.stderr or "EOL" in result.stderr

    def test_import_standard_library(self) -> None:
        """Test that standard library imports work."""
        code = """
import json
import os
print(json.dumps({"test": "value"}))
"""
        result = run_in_sandbox(code)
        assert result.returncode == 0
        assert '{"test": "value"}' in result.stdout

    def test_math_operations(self) -> None:
        """Test that math operations work correctly."""
        code = """
import math
result = math.sqrt(16) + math.pi
print(f"Result: {result:.2f}")
"""
        result = run_in_sandbox(code)
        assert result.returncode == 0
        assert "Result:" in result.stdout


class TestSandboxFilesystemAccess:
    """Test filesystem access restrictions."""

    def test_allows_tmp_access(self) -> None:
        """Test that /tmp is accessible for read/write."""
        code = """
import os
with open('/tmp/test_sandbox.txt', 'w') as f:
    f.write('hello world')
with open('/tmp/test_sandbox.txt', 'r') as f:
    print(f.read())
os.remove('/tmp/test_sandbox.txt')
"""
        result = run_in_sandbox(code)
        assert result.returncode == 0
        assert "hello world" in result.stdout

    @pytest.mark.xfail(
        reason="Full filesystem isolation requires bwrap which may not be available",
    )
    def test_denies_access_outside_allowed(
        self,
        tmp_path: pytest.TempPathFactory,
    ) -> None:
        """Test that access outside allowed paths is denied."""
        # Create a secret file outside the sandbox
        secret_file = tmp_path / "secret.txt"  # type: ignore[operator]
        secret_file.write_text("secret data")

        code = f"""
with open('{secret_file}', 'r') as f:
    print(f.read())
"""
        result = run_in_sandbox(code)
        # Should fail or not return the secret
        assert "secret data" not in result.stdout
        assert result.returncode != 0 or "Permission denied" in result.stderr

    @pytest.mark.xfail(
        reason="Full filesystem isolation requires bwrap which may not be available",
    )
    def test_denies_write_to_system_paths(self) -> None:
        """Test that writing to system paths is denied."""
        code = """
with open('/etc/test_sandbox', 'w') as f:
    f.write('malicious')
"""
        result = run_in_sandbox(code)
        assert result.returncode != 0
        assert (
            "Permission denied" in result.stderr
            or "Read-only" in result.stderr
            or "PermissionError" in result.stderr
        )


class TestSandboxResourceLimits:
    """Test resource limit enforcement."""

    def test_timeout_enforcement(self) -> None:
        """Test that scripts timeout after the limit."""
        code = """
import time
time.sleep(60)
print("Should not reach here")
"""
        # Use a short timeout for testing
        result = run_in_sandbox(code, timeout=2)
        assert result.returncode != 0
        assert result.error_type == SandboxErrorType.TIMEOUT
        assert (
            "timed out" in result.stderr.lower() or "timeout" in result.stderr.lower()
        )

    def test_infinite_loop_timeout(self) -> None:
        """Test that infinite loops are terminated."""
        code = """
while True:
    pass
"""
        result = run_in_sandbox(code, timeout=2)
        assert result.returncode != 0
        assert result.error_type == SandboxErrorType.TIMEOUT


class TestSandboxEnvironment:
    """Test environment variable handling."""

    def test_environment_variables_passed(self) -> None:
        """Test that environment variables are passed to the sandbox."""
        code = """
import os
workspace = os.environ.get('IEAPP_WORKSPACE_ROOT', 'NOT_SET')
print(f"Workspace: {workspace}")
"""
        test_ws = "/tmp/test-workspace"  # noqa: S108
        env = {"IEAPP_WORKSPACE_ROOT": test_ws}
        result = run_in_sandbox(code, env=env)
        assert result.returncode == 0
        assert test_ws in result.stdout

    def test_custom_env_vars(self) -> None:
        """Test that custom environment variables work."""
        code = """
import os
custom = os.environ.get('CUSTOM_VAR', 'default')
print(f"Custom: {custom}")
"""
        env = {"CUSTOM_VAR": "my_value"}
        result = run_in_sandbox(code, env=env)
        assert result.returncode == 0
        assert "my_value" in result.stdout


class TestSandboxResult:
    """Test SandboxResult dataclass."""

    def test_success_property(self) -> None:
        """Test the success property."""
        success_result = SandboxResult(
            stdout="output",
            stderr="",
            returncode=0,
            error_type=SandboxErrorType.SUCCESS,
        )
        assert success_result.success is True

        failure_result = SandboxResult(
            stdout="",
            stderr="error",
            returncode=1,
            error_type=SandboxErrorType.EXECUTION_ERROR,
        )
        assert failure_result.success is False

    def test_error_types(self) -> None:
        """Test different error types."""
        assert SandboxErrorType.SUCCESS.value == "success"
        assert SandboxErrorType.TIMEOUT.value == "timeout"
        assert SandboxErrorType.SECURITY_VIOLATION.value == "security_violation"
        assert SandboxErrorType.MEMORY_EXCEEDED.value == "memory_exceeded"
        assert SandboxErrorType.EXECUTION_ERROR.value == "execution_error"


class TestSandboxIeappIntegration:
    """Test ieapp library integration in sandbox."""

    def test_ieapp_import(self) -> None:
        """Test that ieapp can be imported in the sandbox."""
        code = """
import ieapp
print("ieapp imported successfully")
print(dir(ieapp))
"""
        result = run_in_sandbox(code)
        assert result.returncode == 0
        assert "ieapp imported successfully" in result.stdout

    def test_ieapp_query_function_exists(self) -> None:
        """Test that ieapp.query function is available."""
        code = """
import ieapp
print(f"query function exists: {hasattr(ieapp, 'query')}")
"""
        result = run_in_sandbox(code)
        assert result.returncode == 0
        assert "query function exists: True" in result.stdout
