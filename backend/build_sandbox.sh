#!/bin/bash
set -e

# Download Javy if not present
if [ ! -f "javy" ]; then
    echo "Downloading Javy..."
    curl -L -o javy.gz https://github.com/bytecodealliance/javy/releases/download/v3.0.1/javy-arm-linux-v3.0.1.gz
    gunzip javy.gz
    chmod +x javy
fi

echo "Building sandbox.wasm..."
javy compile src/app/sandbox/runner.js -o src/app/sandbox/sandbox.wasm

echo "Done."
