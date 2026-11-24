# syntax=docker/dockerfile:1.29@sha256:572cebbc2b15e6f84c077742952674889cb63cd5e9b80abdffcbe587b6f19183

FROM denoland/deno:2.9.8@sha256:a0d88a7fcbd44f062cba2748e19df67d30290ad12aa203c7c1e0b12462c483f0 AS frontend-build
WORKDIR /repo
ARG UGOITE_SOURCE_SHA=unknown
ENV UGOITE_SOURCE_SHA=${UGOITE_SOURCE_SHA}
ENV CARGO_TARGET_DIR=target/rust
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl build-essential nodejs \
  && rm -rf /var/lib/apt/lists/*
# Rust compiler authority is rust-toolchain.toml; read the channel instead of
# hardcoding a second version here so drift is visible as a diff.
COPY rust-toolchain.toml ./
RUN channel="$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' rust-toolchain.toml)" \
  && test -n "$channel" \
  && curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "$channel"
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target add wasm32-unknown-unknown
COPY deno.json deno.lock ./
COPY Cargo.toml Cargo.lock ./
COPY frontend ./frontend
COPY scripts ./scripts
COPY crates ./crates
COPY vendor ./vendor
COPY shared ./shared
COPY docs/brand/assets ./docs/brand/assets
RUN cd frontend && deno install --frozen --allow-scripts=npm:@tailwindcss/oxide,npm:esbuild,npm:sharp
RUN bash scripts/build-ugoite-wasm.sh release target/wasm/ugoite_wasm.release.wasm
RUN bash scripts/activate-ugoite-wasm.sh release
RUN UGOITE_STATIC_SPA=true deno task frontend:build
RUN deno run -A frontend/scripts/generate-static-index.ts \
  frontend/.output/public/_build/.vite/manifest.json \
  frontend/.output/public/index.html

FROM rust:1.100-bookworm@sha256:ade39a710ef85f3506cffcfec3e8a24ba090badef1dff2fb798f71d878833fea AS rust-build
WORKDIR /repo
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY vendor ./vendor
RUN cargo build -p ugoite-server --release --locked
RUN cargo build -p ugoite-cli --release --locked

FROM ubuntu:26.04@sha256:d66b366dac61b575bee2aef07e9c26e8070096fbe84a3d4d3eb0da069b5d76db AS runtime-base
WORKDIR /app
ARG UGOITE_SOURCE_SHA=unknown
ENV UGOITE_SOURCE_SHA=${UGOITE_SOURCE_SHA}
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
