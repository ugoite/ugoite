FROM python:3.12-slim-trixie AS builder

# Install build dependencies for nsjail
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
    protobuf-compiler

# Build nsjail
WORKDIR /tmp
RUN git clone https://github.com/google/nsjail.git
WORKDIR /tmp/nsjail
RUN make && cp nsjail /usr/local/bin/

FROM python:3.12-slim-trixie

# Install runtime dependencies for nsjail
RUN apt-get update && apt-get install -y \
    libprotobuf32t64 \
    libnl-route-3-200 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/nsjail /usr/local/bin/nsjail

WORKDIR /app
# Install uv
COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/

# Copy pyproject.toml and uv.lock for dependency installation
COPY ./pyproject.toml .
COPY ./uv.lock .
COPY ./README.md .
COPY --from=module ./ ../ieapp-cli/

# Copy the rest of the backend code
COPY ./src src

ENV UV_LINK_MODE=copy
# Install backend dependencies
RUN uv sync --locked

# Set the entrypoint for the backend application
ENV PYTHONPATH=/app/src
CMD ["uv", "run", "uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
