# syntax=docker/dockerfile:1.7

FROM denoland/deno:2.8.3 AS frontend-build
WORKDIR /repo
ENV CARGO_TARGET_DIR=target/rust
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl build-essential nodejs \
  && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.93.0
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target add wasm32-unknown-unknown
COPY deno.json deno.lock ./
COPY Cargo.toml Cargo.lock ./
COPY frontend ./frontend
COPY scripts ./scripts
COPY crates ./crates
COPY vendor ./vendor
COPY shared ./shared
RUN cd frontend && deno install --allow-scripts=npm:@tailwindcss/oxide,npm:esbuild,npm:sharp
RUN bash scripts/build-ugoite-wasm.sh release target/wasm/ugoite_wasm.release.wasm
RUN bash scripts/activate-ugoite-wasm.sh release
RUN UGOITE_STATIC_SPA=true deno task frontend:build
RUN deno run -A frontend/scripts/generate-static-index.ts \
  frontend/.output/public/_build/.vite/manifest.json \
  frontend/.output/public/index.html

FROM rust:1.96-bookworm AS rust-build
WORKDIR /repo
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY vendor ./vendor
RUN cargo build -p ugoite-server --release --locked
RUN cargo build -p ugoite-cli --release --locked

FROM ubuntu:24.04 AS runtime-base
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --create-home --home-dir /nonexistent --shell /usr/sbin/nologin ugoite \
  && mkdir -p /data /app/static \
  && chown -R ugoite:ugoite /data /app
ENV UGOITE_STATIC_DIR=/app/static
ENV UGOITE_ROOT=/data
ENV UGOITE_SERVER_ADDRESS=0.0.0.0:8000
EXPOSE 8000
USER ugoite
ENTRYPOINT ["ugoite-server"]

FROM runtime-base AS runtime-source
COPY --from=rust-build /repo/target/release/ugoite-server /usr/local/bin/ugoite-server
COPY --from=rust-build /repo/target/release/ugoite /usr/local/bin/ugoite
COPY --from=frontend-build /repo/frontend/.output/public /app/static

FROM runtime-base AS runtime-prebuilt
COPY target/rust/release/ugoite-server /usr/local/bin/ugoite-server
COPY target/rust/release/ugoite /usr/local/bin/ugoite
COPY frontend/.output/public /app/static

# Keep the portable source-build image as the default Dockerfile target.
FROM runtime-source AS runtime
