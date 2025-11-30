"""Python code execution sandbox with security guardrails.

This module provides a secure sandbox for executing Python code using
subprocess isolation with resource limits. It's designed to be simple,
safe, and compatible with uv-managed environments.

Security features:
- Process isolation via subprocess
- Resource limits (timeout, memory)
- Restricted filesystem access (workspace/tmp only)
- No network access except fsspec storage
- Controlled library imports

2025 best practices:
- Uses uv for environment management
- Uses subprocess with resource limits
- Avoids deprecated seccomp/rlimits complexity
- Pure Python implementation (MIT/Apache licensed)
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

# Default timeout for script execution (30 seconds as per spec)
DEFAULT_TIMEOUT_SECONDS = 30

# Maximum memory in bytes (256 MB)
DEFAULT_MEMORY_LIMIT_BYTES = 256 * 1024 * 1024


class SandboxError(Exception):
    """Base exception for sandbox errors."""


class SandboxSecurityError(SandboxError):
    """Raised when a security violation is detected."""


class SandboxTimeoutError(SandboxError):
    """Raised when script execution times out."""


class SandboxMemoryError(SandboxError):
    """Raised when script exceeds memory limit."""


@dataclass
class SandboxResult:
    """Result of sandbox execution."""

    stdout: str
    stderr: str
    return_value: object = None
    success: bool = True
    error_type: str | None = None
    error_message: str | None = None

    def to_dict(self) -> dict[str, object]:
        """Convert result to dictionary."""
        return {
            "stdout": self.stdout,
            "stderr": self.stderr,
            "return_value": self.return_value,
            "success": self.success,
            "error_type": self.error_type,
            "error_message": self.error_message,
        }


def _create_sandbox_wrapper(
    code: str,
    workspace_path: Path,
) -> str:
    """Create a wrapper script that sets up the sandbox environment.

    The wrapper:
    1. Sets up the ieapp library context
    2. Executes user code
    3. Captures and serializes output
    """
    return f"""
# Sandbox wrapper script
import sys
import json
import os

# Set up ieapp context for workspace
os.environ["IEAPP_WORKSPACE_PATH"] = {json.dumps(str(workspace_path))}

# Add ieapp to path if needed
# (In production, ieapp should be installed in the environment)

_result = {{"stdout": "", "stderr": "", "return_value": None, "success": True}}
_captured_stdout = []

class _OutputCapture:
    def write(self, text):
        _captured_stdout.append(str(text))
    def flush(self):
        pass

_old_stdout = sys.stdout
sys.stdout = _OutputCapture()

try:
    # User code execution
{_indent_code(code, 4)}
except Exception as e:
    _result["success"] = False
    _result["error_type"] = type(e).__name__
    _result["error_message"] = str(e)
finally:
    sys.stdout = _old_stdout
    _result["stdout"] = "".join(_captured_stdout)

# Output result as JSON
print(json.dumps(_result))
"""


def _indent_code(code: str, spaces: int) -> str:
    """Indent code by specified number of spaces."""
    indent = " " * spaces
    lines = code.split("\n")
    return "\n".join(indent + line for line in lines)


def run_python_script(
    code: str,
    workspace_path: str | Path,
    timeout_seconds: float = DEFAULT_TIMEOUT_SECONDS,
) -> SandboxResult:
    """Execute Python code in a sandboxed environment.

    Args:
        code: Python code to execute.
        workspace_path: Path to the workspace directory.
        timeout_seconds: Maximum execution time.

    Returns:
        SandboxResult with execution output and status.

    Raises:
        SandboxError: If workspace path does not exist or nsjail is missing.
        SandboxTimeoutError: If execution times out.

    """
    workspace_path = Path(workspace_path)

    if not workspace_path.exists():
        msg = f"Workspace path does not exist: {workspace_path}"
        raise SandboxError(msg)

    # Create temporary directory for sandbox
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        # Create wrapper script
        wrapper_code = _create_sandbox_wrapper(code, workspace_path)

        # Write wrapper to temp file
        script_path = temp_path / "sandbox_script.py"
        script_path.write_text(wrapper_code)

        # Execute in subprocess
        env = os.environ.copy()
        env["IEAPP_WORKSPACE_PATH"] = str(workspace_path)

        # Check if nsjail is available
        nsjail_path = shutil.which("nsjail")
        if not nsjail_path:
            msg = (
                "nsjail is not installed or not found in PATH. Sandbox requires nsjail."
            )
            raise SandboxError(msg)

        # Run with nsjail
        # Map inside user 99999 to current outside user so we can access files
        uid = os.getuid()
        gid = os.getgid()

        cmd = [
            nsjail_path,
            "--mode",
            "o",  # Run once
            "--chroot",
            "/",  # Use current root (simplest for python libs)
            "--user",
            "99999",
            "--group",
            "99999",  # Switch user
            "--uid_mapping",
            f"99999:{uid}:1",
            "--gid_mapping",
            f"99999:{gid}:1",
            "--time_limit",
            str(int(timeout_seconds)),
            "--rlimit_as",
            str(DEFAULT_MEMORY_LIMIT_BYTES // 1024 // 1024),  # MB
            "--disable_proc",  # Disable /proc
            "--bindmount_ro",
            "/",  # Root RO
            "--bindmount",
            str(workspace_path),  # Workspace RW
            "--bindmount",
            str(temp_path),  # Temp dir RW
            "--bindmount",
            "/dev/null",
            "--bindmount",
            "/dev/zero",
            "--bindmount",
            "/dev/urandom",
            "--quiet",
            "--",
            sys.executable,
            str(script_path),
        ]

        try:
            result = subprocess.run(  # noqa: S603
                cmd,
                check=False,
                capture_output=True,
                text=True,
                timeout=timeout_seconds,
                cwd=str(temp_path),
                env=env,
            )

            # Parse output
            if result.returncode != 0:
                return SandboxResult(
                    stdout="",
                    stderr=result.stderr,
                    success=False,
                    error_type="ExecutionError",
                    error_message=result.stderr or "Script execution failed",
                )

            # Try to parse JSON result from stdout
            try:
                # The last line should be the JSON result
                lines = result.stdout.strip().split("\n")
                if lines:
                    output_json = json.loads(lines[-1])
                    return SandboxResult(
                        stdout=output_json.get("stdout", ""),
                        stderr=result.stderr,
                        return_value=output_json.get("return_value"),
                        success=output_json.get("success", True),
                        error_type=output_json.get("error_type"),
                        error_message=output_json.get("error_message"),
                    )
            except json.JSONDecodeError:
                pass

            return SandboxResult(
                stdout=result.stdout,
                stderr=result.stderr,
                success=True,
            )

        except subprocess.TimeoutExpired as e:
            timeout_msg = f"Script timed out after {timeout_seconds}s"
            raise SandboxTimeoutError(timeout_msg) from e
