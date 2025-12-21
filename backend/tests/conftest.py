"""Test configuration."""

import importlib.resources
import importlib.util
import os
import platform
import sys
import tempfile
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

PROJECT_ROOT = Path(__file__).resolve().parents[1]
SRC_ROOT = PROJECT_ROOT / "src"
if str(SRC_ROOT) not in sys.path:
    sys.path.insert(0, str(SRC_ROOT))

from app.main import app


@pytest.fixture(scope="session", autouse=True)
def ensure_sandbox_wasm() -> None:
    """Ensure the sandbox.wasm artifact is present and valid before tests run."""
    arch = platform.machine().lower()

    try:
        ref = importlib.resources.files("ieapp.sandbox") / "sandbox.wasm"
    except (ImportError, TypeError):
        wasm_path = (
            Path(__file__).parent.parent / "src" / "ieapp" / "sandbox" / "sandbox.wasm"
        )
        ctx = None
    else:
        ctx = importlib.resources.as_file(ref)
        wasm_path = ctx.__enter__()

    try:
        needs_build = arch in ("x86_64", "amd64")
        if wasm_path.exists():
            header = wasm_path.read_bytes()[:4]
            # WebAssembly binaries start with 0x00 0x61 0x73 0x6d ("\0asm")
            needs_build = needs_build or header != b"\x00asm"

        if needs_build:
            script = (
                Path(__file__).resolve().parents[2]
                / "ieapp-cli"
                / "scripts"
                / "build_sandbox.py"
            )
            spec = importlib.util.spec_from_file_location("build_sandbox", script)
            if spec is None or spec.loader is None:
                msg = f"Failed to load build_sandbox module from {script}"
                raise RuntimeError(msg)

            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            module.build_sandbox()
    finally:
        if ctx is not None:
            ctx.__exit__(None, None, None)


@pytest.fixture
def test_client(temp_workspace_root: Path) -> TestClient:
    """Create a test client bound to the temporary workspace root."""
    return TestClient(app)


@pytest.fixture
def temp_workspace_root() -> Iterator[Path]:
    """Create a temporary workspace root."""
    with tempfile.TemporaryDirectory() as tmpdir:
        root = Path(tmpdir)
        os.environ["IEAPP_ROOT"] = str(root)
        yield root
        del os.environ["IEAPP_ROOT"]
