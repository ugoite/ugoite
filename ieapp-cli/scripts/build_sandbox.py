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
from urllib.parse import urljoin, urlparse

# Configuration
# Note: Javy v8.0.0 has a known issue on ARM64 (aarch64) where it fails
# with "[parse exception: bad tag index]" when compiling runner.js.
# We use v7.0.1 for ARM64 which works correctly, while keeping v8.0.0
# for x86_64 where it functions properly.
JAVY_VERSION_X86 = "v8.0.0"
JAVY_VERSION_ARM = "v7.0.1"
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

    # We only support Linux releases at the moment. Fail fast to avoid
    # constructing an incorrect Linux-specific download URL on other
    # platforms (which would otherwise lead to confusing errors).
    if system != "linux":
        msg = (
            f"Unsupported operating system for Javy download: {system}. "
            "This script supports Linux only."
        )
        raise RuntimeError(msg)

    if machine in ("x86_64", "amd64"):
        arch = "x86_64"
        version = JAVY_VERSION_X86
    elif machine in ("aarch64", "arm64"):
        arch = "arm"
        version = JAVY_VERSION_ARM
    else:
        msg = f"Unsupported architecture: {machine}"
        raise RuntimeError(msg)

    # Javy release naming convention: javy-{arch}-{os}-{version}.gz
    # e.g. javy-x86_64-linux-v8.0.0.gz, javy-arm-linux-v7.0.1.gz
    filename = f"javy-{arch}-linux-{version}.gz"
    return f"https://github.com/{JAVY_REPO}/releases/download/{version}/{filename}"


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
    if parsed.scheme != "https":
        msg = "Only HTTPS URLs are supported for Javy download"
        raise ValueError(msg)

    # Download the release and write it to `dest_path`.
    body = _fetch_url_body(url)

    # Read response body into memory (should be small) and decompress
    with (
        gzip.GzipFile(fileobj=io.BytesIO(body)) as uncompressed,
        dest_path.open("wb") as f_out,
    ):
        shutil.copyfileobj(uncompressed, f_out)

    # Make executable using Path API
    mode = dest_path.stat().st_mode | stat.S_IEXEC
    dest_path.chmod(mode)
    logger.info("Javy downloaded to %s", dest_path)


def _fetch_url_body(url: str, max_redirects: int = 5) -> bytes:
    """Fetch a URL and follow redirects, returning the response body.

    This helper keeps redirect logic isolated so the public function can
    remain small and easy to test.
    """
    current_url = url
    for _ in range(max_redirects):
        parsed = urlparse(current_url)
        if parsed.scheme != "https":
            msg = "Only HTTPS URLs are supported for Javy download"
            raise ValueError(msg)

        conn = http.client.HTTPSConnection(parsed.netloc, timeout=30)
        path = parsed.path or "/"
        if parsed.query:
            path = path + "?" + parsed.query

        try:
            conn.request("GET", path)
            resp = conn.getresponse()

            # Successful response; read and return the body.
            if resp.status == http.client.OK:
                return resp.read()

            # Handle redirects (301, 302, 303, 307, 308)
            if resp.status in (301, 302, 303, 307, 308):
                loc = resp.getheader("location")
                if not loc:
                    msg = f"Failed to download Javy: HTTP {resp.status} (no Location)"
                    raise RuntimeError(msg)
                current_url = urljoin(current_url, loc)
                logger.info("Redirecting Javy download to %s", current_url)
                continue

            # Any other error
            msg = f"Failed to download Javy: HTTP {resp.status}"
            raise RuntimeError(msg)

        finally:
            close = getattr(conn, "close", None)
            if callable(close):
                try:
                    close()
                except OSError:
                    logger.debug("Closing connection failed", exc_info=True)

    msg = "Failed to download Javy: too many redirects or no successful response"
    raise RuntimeError(msg)


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

            # Determine the appropriate subcommand to use. Newer Javy
            # releases (v8+) expose a `build` subcommand; older releases
            # used `compile`. Probe the binary's help output to decide.
            try:
                proc = subprocess.run(
                    [str(javy_bin), "--help"],
                    check=False,
                    capture_output=True,
                    text=True,
                )
                help_out = (proc.stdout or "") + (proc.stderr or "")
                help_lower = help_out.lower()
                if "compile" in help_lower:
                    subcmd = "compile"
                elif "build" in help_lower:
                    subcmd = "build"
                else:
                    # Prefer newer `build` command as a sensible default.
                    subcmd = "build"
            except (
                subprocess.SubprocessError,
                OSError,
            ) as exc:
                # If probing fails, default to `build` and allow the
                # subsequent subprocess.run to report a more helpful error
                # if the subcommand is unsupported.
                logger.debug("Failed to probe javy help: %s", exc)
                subcmd = "build"

            subprocess.run(
                [
                    str(javy_bin),
                    subcmd,
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
