import subprocess
import tempfile
import os
import sys
import resource
from dataclasses import dataclass

@dataclass
class SandboxResult:
    stdout: str
    stderr: str
    returncode: int

def set_limits():
    # Limit CPU time to 5 seconds
    try:
        resource.setrlimit(resource.RLIMIT_CPU, (5, 5))
    except ValueError:
        pass # Ignore if we can't set limits
    
    # Limit memory to 512MB
    try:
        resource.setrlimit(resource.RLIMIT_AS, (512 * 1024 * 1024, 512 * 1024 * 1024))
    except ValueError:
        pass

def run_in_sandbox(code: str, env: dict[str, str] | None = None) -> SandboxResult:
    with tempfile.NamedTemporaryFile(mode='w', suffix='.py', delete=False) as f:
        f.write(code)
        script_path = f.name

    try:
        run_env = os.environ.copy()
        if env:
            run_env.update(env)

        # Fallback to simple subprocess for dev environment where bwrap/nsjail are not available
        result = subprocess.run(
            [sys.executable, script_path],
            capture_output=True,
            text=True,
            preexec_fn=set_limits,
            timeout=10,
            cwd="/tmp", # Run in tmp
            env=run_env
        )
        return SandboxResult(
            stdout=result.stdout,
            stderr=result.stderr,
            returncode=result.returncode
        )
    except subprocess.TimeoutExpired:
        return SandboxResult(stdout="", stderr="Timeout", returncode=124)
    except Exception as e:
        return SandboxResult(stdout="", stderr=str(e), returncode=1)
    finally:
        if os.path.exists(script_path):
            os.unlink(script_path)
