#!/usr/bin/env bash
set -euo pipefail

# Build the Wasm sandbox artifact using Javy.
# This script will download a release of Javy for the current architecture if
# one is not present locally. When possible it attempts to verify a SHA256
# checksum published alongside the release asset.

JAVY_VERSION="${JAVY_VERSION:-v3.0.1}"

arch="$(uname -m)"
case "$arch" in
    x86_64|amd64)
        asset_arch="x86_64"
        ;;
    aarch64|arm64)
        asset_arch="arm"
        ;;
    *)
        echo "Unsupported architecture: $arch" >&2
        exit 1
        ;;
esac

# Download Javy if not present
if [ ! -f "javy" ]; then
    echo "Downloading Javy..."
    asset_url="https://github.com/bytecodealliance/javy/releases/download/${JAVY_VERSION}/javy-${asset_arch}-linux-${JAVY_VERSION}.gz"
    curl -fsSL -o javy.gz "$asset_url"

    # Try to verify a SHA256 checksum if the release provides one.
    checksum_url1="${asset_url}.sha256"
    checksum_url2="${asset_url}.sha256sum"
    checksum_ok=false

    if curl -fsSL -o javy.gz.sha256 "$checksum_url1"; then
        expected=$(awk '{print $1}' javy.gz.sha256 | head -n1)
        actual=$(sha256sum javy.gz | awk '{print $1}')
        if [ "$expected" = "$actual" ]; then
            checksum_ok=true
        else
            echo "ERROR: checksum mismatch for javy.gz" >&2
            exit 1
        fi
    elif curl -fsSL -o javy.gz.sha256 "$checksum_url2"; then
        expected=$(awk '{print $1}' javy.gz.sha256 | head -n1)
        actual=$(sha256sum javy.gz | awk '{print $1}')
        if [ "$expected" = "$actual" ]; then
            checksum_ok=true
        else
            echo "ERROR: checksum mismatch for javy.gz" >&2
            exit 1
        fi
    else
        echo "Warning: no checksum available for ${asset_url}; proceeding without verification" >&2
    fi

    gunzip -f javy.gz
    chmod +x javy
fi

echo "Building sandbox.wasm..."
if command -v javy >/dev/null 2>&1; then
    javy compile src/app/sandbox/runner.js -o src/app/sandbox/sandbox.wasm
else
    ./javy compile src/app/sandbox/runner.js -o src/app/sandbox/sandbox.wasm
fi

echo "Done."
