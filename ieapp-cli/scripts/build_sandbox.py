#!/usr/bin/env python3
"""
Build script for the WebAssembly sandbox.
This script downloads a temporary Javy binary to compile runner.js into sandbox.wasm.
It avoids requiring the user to install Javy manually.
"""

import gzip
import os
import platform
import shutil
import stat
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

# Configuration
JAVY_VERSION = "v3.0.1"  # Keep consistent with previous version for stability
JAVY_REPO = "bytecodealliance/javy"


def get_javy_url():
    """Determine the correct Javy download URL for the current platform."""
    system = platform.system().lower()
    machine = platform.machine().lower()

    if system != "linux":
        # For now, we only support Linux as per the original script,
        # but we can easily add Mac/Windows support if needed.
        # The dev container is Linux.
        print(f"Warning: This build script is optimized for Linux. Detected: {system}")

    if machine in ("x86_64", "amd64"):
        arch = "x86_64"
    elif machine in ("aarch64", "arm64"):
        arch = "arm"
    else:
        raise RuntimeError(f"Unsupported architecture: {machine}")

    # Javy release naming convention: javy-{arch}-{os}-{version}.gz
    # e.g. javy-x86_64-linux-v3.0.1.gz
    filename = f"javy-{arch}-linux-{JAVY_VERSION}.gz"
    url = f"https://github.com/{JAVY_REPO}/releases/download/{JAVY_VERSION}/{filename}"
    return url


def download_and_extract_javy(dest_path):
    """Download Javy and extract it to the destination path."""
    url = get_javy_url()
    print(f"Downloading Javy from {url}...")

    with urllib.request.urlopen(url) as response:
        with gzip.GzipFile(fileobj=response) as uncompressed:
            with open(dest_path, "wb") as f_out:
                shutil.copyfileobj(uncompressed, f_out)

    # Make executable
    st = os.stat(dest_path)
    os.chmod(dest_path, st.st_mode | stat.S_IEXEC)
    print(f"Javy downloaded to {dest_path}")


def build_sandbox():
    """Compile runner.js to sandbox.wasm using a temporary Javy binary."""
    # Paths
    script_dir = Path(__file__).parent
    # ieapp-cli/scripts -> ieapp-cli/src/ieapp/sandbox
    sandbox_dir = script_dir.parent / "src" / "ieapp" / "sandbox"
    runner_js = sandbox_dir / "runner.js"
    output_wasm = sandbox_dir / "sandbox.wasm"

    if not runner_js.exists():
        raise FileNotFoundError(f"runner.js not found at {runner_js}")

    with tempfile.TemporaryDirectory() as tmp_dir:
        javy_bin = Path(tmp_dir) / "javy"

        try:
            download_and_extract_javy(javy_bin)

            print(f"Compiling {runner_js} to {output_wasm}...")
            subprocess.run(
                [str(javy_bin), "compile", str(runner_js), "-o", str(output_wasm)],
                check=True,
            )
            print("Build successful.")

        except Exception as e:
            print(f"Error during build: {e}")
            sys.exit(1)


if __name__ == "__main__":
    build_sandbox()
