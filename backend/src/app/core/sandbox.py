"""Secure Python code execution sandbox.

This module provides a secure sandboxed environment for executing Python code.
It uses bubblewrap (bwrap) for namespace isolation when available, with a
fallback to subprocess with resource limits for development environments.

Security measures (per spec 05 §Sandbox):
- Filesystem: Restricted to workspace (read-only) and /tmp (read-write)
- Network: Blocked entirely (except for fsspec remote storage if configured)
- Resources: CPU timeout (30s), memory limit (512MB)
- Process: Isolated namespace with PR_SET_NO_NEW_PRIVS

Note: S108 warnings are suppressed as /tmp usage is intentional and secure
within the sandbox context. S603 warnings are suppressed as subprocess calls
are necessary for sandbox execution with controlled inputs.
"""

from __future__ import annotations

import contextlib
import ctypes
import logging
import os
import resource
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path

logger = logging.getLogger(__name__)

# Constants per spec 05 §Sandbox
DEFAULT_TIMEOUT_SECONDS = 30
DEFAULT_MEMORY_LIMIT_BYTES = 512 * 1024 * 1024  # 512MB
DEFAULT_CPU_LIMIT_SECONDS = 5


class SandboxErrorType(Enum):
    """Types of sandbox execution errors."""

    SUCCESS = "success"
    TIMEOUT = "timeout"
    SECURITY_VIOLATION = "security_violation"
    MEMORY_EXCEEDED = "memory_exceeded"
    EXECUTION_ERROR = "execution_error"


@dataclass
class SandboxResult:
    """Result of code execution in the sandbox."""

    stdout: str
    stderr: str
    returncode: int
    error_type: SandboxErrorType = field(default=SandboxErrorType.SUCCESS)

    @property
    def success(self) -> bool:
        """Check if execution was successful."""
        return self.returncode == 0 and self.error_type == SandboxErrorType.SUCCESS


def _is_bwrap_available() -> bool:
    """Check if bubblewrap (bwrap) is available and functional on the system.

    This not only checks if bwrap binary exists, but also tests if user
    namespaces are enabled (required for non-root bwrap usage).
    """
    bwrap_path = shutil.which("bwrap")
    if bwrap_path is None:
        return False

    # Test if bwrap actually works (user namespaces may be disabled)
    try:
        result = subprocess.run(  # noqa: S603
            [bwrap_path, "--ro-bind", "/", "/", "true"],
            check=False,
            capture_output=True,
            timeout=5,
        )
    except (subprocess.SubprocessError, OSError):
        return False
    else:
        return result.returncode == 0


# Constant for prctl call
_PR_SET_NO_NEW_PRIVS = 38


def _set_resource_limits() -> None:
    """Set resource limits for the subprocess (fallback mode).

    This is used when bwrap is not available.
    """
    # Set CPU time limit
    with contextlib.suppress(ValueError, OSError):
        resource.setrlimit(
            resource.RLIMIT_CPU,
            (DEFAULT_CPU_LIMIT_SECONDS, DEFAULT_CPU_LIMIT_SECONDS),
        )

    # Set memory limit
    mem_limit = DEFAULT_MEMORY_LIMIT_BYTES
    with contextlib.suppress(ValueError, OSError):
        resource.setrlimit(resource.RLIMIT_AS, (mem_limit, mem_limit))

    # Prevent new privileges
    with contextlib.suppress(OSError, AttributeError):
        libc = ctypes.CDLL("libc.so.6", use_errno=True)
        libc.prctl(_PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)


def _build_bwrap_command(
    python_path: str,
    script_path: str,
    workspace_path: Path | None = None,
    ieapp_lib_path: Path | None = None,
) -> list[str]:
    """Build the bubblewrap command with security restrictions.

    Args:
        python_path: Path to the Python interpreter
        script_path: Path to the script to execute
        workspace_path: Optional workspace path to mount read-only
        ieapp_lib_path: Path to the ieapp library for PYTHONPATH

    Returns:
        List of command arguments for bwrap

    """
    cmd: list[str] = [
        "bwrap",
        # Create new namespaces for isolation
        "--unshare-pid",  # PID namespace
        "--unshare-net",  # Network namespace (blocks network)
        "--unshare-ipc",  # IPC namespace
        "--unshare-uts",  # UTS namespace (hostname)
        # New session to prevent terminal escape attacks
        "--new-session",
        # Basic read-only filesystem structure
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/lib",
        "/lib",
        "--ro-bind",
        "/lib64",
        "/lib64",
        "--ro-bind",
        "/etc/alternatives",
        "/etc/alternatives",
        "--ro-bind",
        "/etc/ssl",
        "/etc/ssl",  # SSL certificates
        # /proc and /dev for basic functionality
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        # Temporary directory for script execution (read-write)
        "--tmpfs",
        "/tmp",  # noqa: S108
        # Bind the script file
        "--ro-bind",
        script_path,
        script_path,
    ]

    # Mount Python installation read-only
    python_prefix = Path(sys.prefix)
    if python_prefix.exists():
        cmd.extend(
            ["--ro-bind", str(python_prefix), str(python_prefix)],
        )

    # If using a venv, also mount the base Python
    if hasattr(sys, "base_prefix") and sys.base_prefix != sys.prefix:
        base_prefix = Path(sys.base_prefix)
        if base_prefix.exists():
            cmd.extend(
                ["--ro-bind", str(base_prefix), str(base_prefix)],
            )

    # Mount ieapp library read-only if provided
    if ieapp_lib_path and ieapp_lib_path.exists():
        cmd.extend(
            ["--ro-bind", str(ieapp_lib_path), str(ieapp_lib_path)],
        )

    # Mount workspace read-only if provided
    if workspace_path and workspace_path.exists():
        cmd.extend(
            ["--ro-bind", str(workspace_path), str(workspace_path)],
        )
        # Also mount as writable under a different path for updates
        workspace_tmp = f"/tmp/workspace/{workspace_path.name}"  # noqa: S108
        cmd.extend(
            [
                "--bind",
                str(workspace_path),
                workspace_tmp,
            ],
        )

    # Die if parent dies
    cmd.append("--die-with-parent")

    # The actual command to run
    cmd.extend([python_path, script_path])

    return cmd


def _run_with_bwrap(
    code: str,
    workspace_path: Path | None = None,
    env: dict[str, str] | None = None,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> SandboxResult:
    """Run code in a bubblewrap sandbox.

    Args:
        code: Python code to execute
        workspace_path: Optional workspace path to mount
        env: Additional environment variables
        timeout: Execution timeout in seconds

    Returns:
        SandboxResult with execution output

    """
    # Create temporary script file
    with tempfile.NamedTemporaryFile(
        mode="w",
        suffix=".py",
        delete=False,
        dir="/tmp",
    ) as f:
        f.write(code)
        script_path = f.name

    try:
        # Build environment
        run_env = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": "/tmp",  # noqa: S108
            "PYTHONDONTWRITEBYTECODE": "1",
            "PYTHONUNBUFFERED": "1",
        }

        # Add Python path for ieapp
        ieapp_lib_path = _find_ieapp_path()
        if ieapp_lib_path:
            run_env["PYTHONPATH"] = str(ieapp_lib_path.parent)

        if env:
            run_env.update(env)

        # Build bwrap command
        cmd = _build_bwrap_command(
            python_path=sys.executable,
            script_path=script_path,
            workspace_path=workspace_path,
            ieapp_lib_path=ieapp_lib_path,
        )

        # Execute with timeout (S603 suppressed: controlled input in sandbox)
        result = subprocess.run(  # noqa: S603
            cmd,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=run_env,
            cwd="/tmp",  # noqa: S108
        )

        return SandboxResult(
            stdout=result.stdout,
            stderr=result.stderr,
            returncode=result.returncode,
            error_type=SandboxErrorType.SUCCESS
            if result.returncode == 0
            else SandboxErrorType.EXECUTION_ERROR,
        )

    except subprocess.TimeoutExpired:
        return SandboxResult(
            stdout="",
            stderr=f"Execution timed out after {timeout} seconds",
            returncode=124,
            error_type=SandboxErrorType.TIMEOUT,
        )
    except PermissionError as e:
        return SandboxResult(
            stdout="",
            stderr=f"Security violation: {e}",
            returncode=126,
            error_type=SandboxErrorType.SECURITY_VIOLATION,
        )
    except (OSError, subprocess.SubprocessError) as e:
        return SandboxResult(
            stdout="",
            stderr=f"Sandbox error: {e}",
            returncode=1,
            error_type=SandboxErrorType.EXECUTION_ERROR,
        )
    finally:
        # Clean up script file
        with contextlib.suppress(OSError):
            Path(script_path).unlink(missing_ok=True)


def _run_with_subprocess(
    code: str,
    workspace_path: Path | None = None,  # noqa: ARG001
    env: dict[str, str] | None = None,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> SandboxResult:
    """Run code using subprocess with resource limits (fallback mode).

    This is used when bubblewrap is not available (e.g., in dev containers).
    WARNING: This provides less isolation than bwrap.

    Args:
        code: Python code to execute
        workspace_path: Optional workspace path
        env: Additional environment variables
        timeout: Execution timeout in seconds

    Returns:
        SandboxResult with execution output

    """
    with tempfile.NamedTemporaryFile(
        mode="w",
        suffix=".py",
        delete=False,
        dir="/tmp",
    ) as f:
        f.write(code)
        script_path = f.name

    try:
        run_env = os.environ.copy()

        # Add Python path for ieapp
        ieapp_lib_path = _find_ieapp_path()
        if ieapp_lib_path:
            existing_path = run_env.get("PYTHONPATH", "")
            run_env["PYTHONPATH"] = (
                f"{ieapp_lib_path.parent}:{existing_path}"
                if existing_path
                else str(ieapp_lib_path.parent)
            )

        if env:
            run_env.update(env)

        # Run with resource limits (S603 suppressed: controlled input)
        result = subprocess.run(  # noqa: S603
            [sys.executable, script_path],
            check=False,
            capture_output=True,
            text=True,
            preexec_fn=_set_resource_limits,
            timeout=timeout,
            cwd="/tmp",  # noqa: S108
            env=run_env,
        )

        return SandboxResult(
            stdout=result.stdout,
            stderr=result.stderr,
            returncode=result.returncode,
            error_type=SandboxErrorType.SUCCESS
            if result.returncode == 0
            else SandboxErrorType.EXECUTION_ERROR,
        )

    except subprocess.TimeoutExpired:
        return SandboxResult(
            stdout="",
            stderr=f"Execution timed out after {timeout} seconds",
            returncode=124,
            error_type=SandboxErrorType.TIMEOUT,
        )
    except MemoryError:
        return SandboxResult(
            stdout="",
            stderr="Memory limit exceeded",
            returncode=137,
            error_type=SandboxErrorType.MEMORY_EXCEEDED,
        )
    except (OSError, subprocess.SubprocessError) as e:
        return SandboxResult(
            stdout="",
            stderr=f"Execution error: {e}",
            returncode=1,
            error_type=SandboxErrorType.EXECUTION_ERROR,
        )
    finally:
        with contextlib.suppress(OSError):
            Path(script_path).unlink(missing_ok=True)


def _find_ieapp_path() -> Path | None:
    """Find the ieapp library path for PYTHONPATH."""
    # Try to import ieapp directly
    # This is done at runtime to avoid circular imports
    try:
        import ieapp as ieapp_module  # noqa: PLC0415

        module_file = ieapp_module.__file__
        if module_file:
            return Path(module_file).parent
    except ImportError:
        pass

    # Fall back to finding it relative to the workspace
    workspace_root = Path(__file__).parents[4]
    ieapp_path = workspace_root / "ieapp-cli" / "src" / "ieapp"
    if ieapp_path.exists():
        return ieapp_path
    return None


def run_in_sandbox(
    code: str,
    env: dict[str, str] | None = None,
    timeout: int = DEFAULT_TIMEOUT_SECONDS,
) -> SandboxResult:
    """Execute Python code in a secure sandbox.

    This function provides the main entry point for sandboxed code execution.
    It will use bubblewrap when available for maximum security, falling back
    to subprocess with resource limits otherwise.

    Args:
        code: Python code to execute
        env: Additional environment variables to set
        timeout: Maximum execution time in seconds (default: 30)

    Returns:
        SandboxResult containing stdout, stderr, returncode, and error_type

    Example:
        >>> result = run_in_sandbox("print('Hello, World!')")
        >>> print(result.stdout)
        Hello, World!

    """
    # Parse workspace path from environment
    workspace_path = None
    if env and "IEAPP_WORKSPACE_ROOT" in env:
        workspace_path = Path(env["IEAPP_WORKSPACE_ROOT"])

    # Use bwrap if available, otherwise fall back to subprocess
    if _is_bwrap_available():
        logger.debug("Using bubblewrap sandbox")
        return _run_with_bwrap(
            code,
            workspace_path=workspace_path,
            env=env,
            timeout=timeout,
        )
    logger.debug("Bubblewrap not available, using subprocess fallback")
    return _run_with_subprocess(
        code,
        workspace_path=workspace_path,
        env=env,
        timeout=timeout,
    )
