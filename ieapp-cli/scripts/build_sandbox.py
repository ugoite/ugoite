#!/usr/bin/env python3
"""Build script for the WebAssembly sandbox.

This script downloads a temporary Javy binary to compile ``runner.js`` into
``sandbox.wasm``. It avoids requiring the user to install Javy manually.
"""

from __future__ import annotations

import gzip
import http.client
import io
import logging
import os
import platform
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import urlparse

# Configuration
JAVY_VERSION = "v3.0.1"  # Keep consistent with previous version for stability
JAVY_REPO = "bytecodealliance/javy"

logger = logging.getLogger(__name__)


def _raise(exc: BaseException) -> None:
    """Raise exceptions from expressions for linting."""
    raise exc


def get_javy_url() -> str:
    """Determine the correct Javy download URL for the current platform.

    Returns:
        The download URL for the appropriate Javy release.

    """
    system = platform.system().lower()
    machine = platform.machine().lower()

    if system != "linux":
        # For now, we only support Linux as per the original script,
        # but we can easily add Mac/Windows support if needed.
        # The dev container is Linux.
        logger.warning(
            "This build script is optimized for Linux. Detected: %s",
            system,
        )

    if machine in ("x86_64", "amd64"):
        arch = "x86_64"
    elif machine in ("aarch64", "arm64"):
        arch = "arm"
    else:
        msg = f"Unsupported architecture: {machine}"
        raise RuntimeError(msg)

    # Javy release naming convention: javy-{arch}-{os}-{version}.gz
    # e.g. javy-x86_64-linux-v3.0.1.gz
    filename = f"javy-{arch}-linux-{JAVY_VERSION}.gz"
    return f"https://github.com/{JAVY_REPO}/releases/download/{JAVY_VERSION}/{filename}"


def download_and_extract_javy(dest_path: Path) -> None:
    """Download Javy and extract it to the destination path.

    Args:
        dest_path: The destination path where the Javy binary will be written.

    """
    url = get_javy_url()
    logger.info("Downloading Javy from %s", url)

    # Validate URL scheme and host before opening to avoid unexpected
    # schemes (like file:) or hosts. We only accept HTTPS downloads from
    # the GitHub releases domain used by `get_javy_url`.
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.netloc != "github.com":
        msg = "Unsupported URL for Javy download"
        raise ValueError(msg)

    # Use an explicit HTTPS connection and in-memory buffer to fetch the
    # gzip-compressed Javy release. This avoids calling ``urllib.request``'s
    # high-level open functions directly with an arbitrary URL (which can
    # trigger security audits like S310), while still performing proper
    # scheme/host validation above.
    conn = http.client.HTTPSConnection(parsed.netloc, timeout=30)
    path = parsed.path or "/"
    if parsed.query:
        path = path + "?" + parsed.query

    conn.request("GET", path)
    resp = conn.getresponse()
    if resp.status != http.client.OK:
        msg = f"Failed to download Javy: HTTP {resp.status}"
        raise RuntimeError(msg)

    # Read response body into memory (should be small) and decompress
    body = resp.read()
    with (
        gzip.GzipFile(fileobj=io.BytesIO(body)) as uncompressed,
        dest_path.open("wb") as f_out,
    ):
        shutil.copyfileobj(uncompressed, f_out)

    # Make executable using Path API
    mode = dest_path.stat().st_mode | stat.S_IEXEC
    dest_path.chmod(mode)
    logger.info("Javy downloaded to %s", dest_path)


def build_sandbox() -> None:
    """Compile ``runner.js`` to ``sandbox.wasm`` using a temporary Javy binary."""
    # Paths
    script_dir = Path(__file__).parent
    # ieapp-cli/scripts -> ieapp-cli/src/ieapp/sandbox
    sandbox_dir = script_dir.parent / "src" / "ieapp" / "sandbox"
    runner_js = sandbox_dir / "runner.js"
    output_wasm = sandbox_dir / "sandbox.wasm"

    if not runner_js.exists():
        msg = f"runner.js not found at {runner_js}"
        raise FileNotFoundError(msg)

    with tempfile.TemporaryDirectory() as tmp_dir:
        javy_bin = Path(tmp_dir) / "javy"

        try:
            download_and_extract_javy(javy_bin)

            logger.info(
                "Compiling %s to %s...",
                runner_js,
                output_wasm,
            )
            # Ensure the downloaded Javy binary exists and is executable
            if not javy_bin.exists():
                msg = f"Javy binary not found at {javy_bin}"
                _raise(FileNotFoundError(msg))
            if not os.access(javy_bin, os.X_OK):
                msg = f"Javy binary is not executable: {javy_bin}"
                _raise(PermissionError(msg))

            subprocess.run(  # noqa: S603 - binary verified above
                [
                    str(javy_bin),
                    "compile",
                    str(runner_js),
                    "-o",
                    str(output_wasm),
                ],
                check=True,
            )
            logger.info("Build successful.")

        except Exception:
            logger.exception("Error during build")
            sys.exit(1)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    build_sandbox()
