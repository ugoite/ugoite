FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

# Install nsjail build dependencies and runtime requirements
RUN apt-get update && apt-get install -y \
    autoconf \
    bison \
    flex \
    gcc \
    g++ \
    git \
    libprotobuf-dev \
    libnl-route-3-dev \
    libtool \
    make \
    pkg-config \
    protobuf-compiler \
    uidmap \
    && rm -rf /var/lib/apt/lists/*

# Build and install nsjail
RUN git clone --depth 1 https://github.com/google/nsjail.git /tmp/nsjail \
    && cd /tmp/nsjail \
    && make \
    && cp nsjail /usr/local/bin/ \
    && rm -rf /tmp/nsjail

# Verify nsjail installation
RUN which nsjail && nsjail --help | head -1
