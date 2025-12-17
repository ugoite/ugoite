#!/bin/bash
set -e

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
        curl -L -o javy.gz "https://github.com/bytecodealliance/javy/releases/download/${JAVY_VERSION}/javy-${asset_arch}-linux-${JAVY_VERSION}.gz"
    gunzip javy.gz
    chmod +x javy
fi

echo "Building sandbox.wasm..."
if command -v javy >/dev/null 2>&1; then
    javy compile src/app/sandbox/runner.js -o src/app/sandbox/sandbox.wasm
else
    ./javy compile src/app/sandbox/runner.js -o src/app/sandbox/sandbox.wasm
fi

echo "Done."
