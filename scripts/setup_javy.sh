#!/bin/bash
set -e

JAVY_VERSION="v3.0.1"
JAVY_URL="https://github.com/bytecodealliance/javy/releases/download/${JAVY_VERSION}/javy-arm-linux-${JAVY_VERSION}.gz"
INSTALL_DIR="/usr/local/bin"

if [ ! -w "$INSTALL_DIR" ]; then
    echo "Cannot write to $INSTALL_DIR. Using sudo..."
    SUDO="sudo"
else
    SUDO=""
fi

echo "Downloading Javy ${JAVY_VERSION}..."
curl -L -o javy.gz "$JAVY_URL"
gunzip javy.gz
chmod +x javy

echo "Installing to $INSTALL_DIR..."
$SUDO mv javy "$INSTALL_DIR/javy"

echo "Javy installed successfully."
javy --version
