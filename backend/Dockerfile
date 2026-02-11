FROM python:3.14-slim-trixie

WORKDIR /app
# Install uv
COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/

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

# Copy pyproject.toml and uv.lock for dependency installation
COPY ./pyproject.toml .
COPY ./uv.lock .
COPY ./README.md .
COPY --from=core ./ ../ugoite-core/
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
