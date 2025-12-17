#!/bin/bash
set -e

# Download Javy if not present
if [ ! -f "javy" ]; then
    echo "Downloading Javy..."
    ARCH=$(uname -m)
    if [ "$ARCH" = "x86_64" ]; then
        JAVY_URL="https://github.com/bytecodealliance/javy/releases/download/v3.0.1/javy-x86_64-linux-v3.0.1.gz"
    elif [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
        JAVY_URL="https://github.com/bytecodealliance/javy/releases/download/v3.0.1/javy-arm-linux-v3.0.1.gz"
    else
        echo "Unsupported architecture: $ARCH"
        exit 1
    fi
    
    curl -L -o javy.gz "$JAVY_URL"
    gunzip javy.gz
    chmod +x javy
fi

echo "Building sandbox.wasm..."
javy compile src/app/sandbox/runner.js -o src/app/sandbox/sandbox.wasm

echo "Done."
