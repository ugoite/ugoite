# syntax=docker/dockerfile:1.7

FROM denoland/deno:2.8.3 AS frontend-build
WORKDIR /repo
RUN apt-get update \
  && apt-get install -y --no-install-recommends nodejs \
  && rm -rf /var/lib/apt/lists/*
COPY deno.json deno.lock ./
COPY frontend ./frontend
COPY docsite ./docsite
COPY docs ./docs
COPY shared ./shared
RUN cd frontend && deno install --allow-scripts=npm:@tailwindcss/oxide,npm:esbuild,npm:sharp
RUN cd docsite && deno install --allow-scripts=npm:@tailwindcss/oxide,npm:esbuild,npm:sharp
RUN UGOITE_STATIC_SPA=true deno task frontend:build
RUN deno run -A frontend/scripts/generate-static-index.ts \
  frontend/.output/public/_build/.vite/manifest.json \
  frontend/.output/public/index.html
RUN deno task docsite:build

FROM rust:1.93-bookworm AS rust-build
WORKDIR /repo
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY vendor ./vendor
RUN cargo build -p ugoite-server --release --locked
RUN cargo build -p ugoite-cli --release --locked

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --create-home --home-dir /nonexistent --shell /usr/sbin/nologin ugoite \
  && mkdir -p /data /app/static \
  && chown -R ugoite:ugoite /data /app
COPY --from=rust-build /repo/target/release/ugoite-server /usr/local/bin/ugoite-server
COPY --from=rust-build /repo/target/release/ugoite /usr/local/bin/ugoite
COPY --from=frontend-build /repo/frontend/.output/public /app/static
ENV UGOITE_STATIC_DIR=/app/static
ENV UGOITE_ROOT=/data
ENV UGOITE_SERVER_ADDRESS=0.0.0.0:8000
EXPOSE 8000
USER ugoite
ENTRYPOINT ["ugoite-server"]
