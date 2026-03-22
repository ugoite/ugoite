FROM python:3.14.3-slim-trixie@sha256:fb83750094b46fd6b8adaa80f66e2302ecbe45d513f6cece637a841e1025b4ca

WORKDIR /app
# Install uv
COPY --from=ghcr.io/astral-sh/uv:0.10.10 /uv /uvx /bin/

# Build essentials and Rust toolchain for compiling ugoite-core (Rust native modules)
RUN apt-get update \
	&& apt-get install -y --no-install-recommends \
		build-essential \
		pkg-config \
		libssl-dev \
		curl \
		ca-certificates \
	&& rm -rf /var/lib/apt/lists/* \
	&& curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.88.0

ENV PATH="/root/.cargo/bin:${PATH}"

# Copy dependency metadata and workspace crates needed for backend installation
COPY ./pyproject.toml .
COPY ./uv.lock .
COPY ./README.md .
COPY --from=core ./ ../ugoite-core/
COPY --from=minimum ./ ../ugoite-minimum/
COPY --from=module ./ ../ugoite-cli/

# Copy the rest of the backend code
COPY ./src src

ENV UV_LINK_MODE=copy
ENV PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
# Install backend dependencies
RUN uv sync --locked

# Set the entrypoint for the backend application
ENV PYTHONPATH=/app/src
CMD ["uv", "run", "uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
