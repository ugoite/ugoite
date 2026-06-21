# Technology Stack

## Overview

Ugoite uses a modern stack optimized for local-first operation and AI integration.
Deployment packaging keeps the runtime topology portable across container
interfaces. The repository-owned deployment artifacts are
`docker-compose.release.yaml` for published Compose installs and `charts/ugoite`
for Kubernetes installs; both package the same backend + frontend images, keep
the backend storage contract rooted at `/data`, and preserve frontend-to-backend
service wiring instead of hard-coding a host-specific endpoint. The published
backend image and Helm deployments also default to non-root/container-hardened
runtime settings so browser-oriented installs do not start with root-only privileges.

## Core Technologies

### ugoite-minimum (Rust Crate)

| Technology | Version | Purpose |
|------------|---------|---------|
| Rust | 1.75+ | Portable domain/runtime-agnostic core |
| [serde](https://serde.rs/) | Latest | Data model serialization |
| [async-trait](https://docs.rs/async-trait) | Latest | Storage abstraction traits |
| [sha2](https://docs.rs/sha2) | Latest | Integrity primitives |

### ugoite-core (Rust Crate)

| Technology | Version | Purpose |
|------------|---------|---------|
| Rust | 1.75+ | Core language |
| [OpenDAL](https://opendal.apache.org/) | Latest | Storage adapter implementation (local, S3, GCS, memory) |
| [serde](https://serde.rs/) | Latest | JSON serialization |
| [wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/) | Latest | WebAssembly bindings (future) |

### ugoite-cli (Rust)

| Technology | Version | Purpose |
|------------|---------|---------|
| Rust | 1.75+ | CLI runtime |
| [clap](https://docs.rs/clap/) | Latest | Command parsing and help output |
| [reqwest](https://docs.rs/reqwest/) | Latest | Backend/API routing |
| `ugoite-core` crate | Workspace path dependency | Shared core integration |

### ugoite-server (Rust/Axum)

| Technology | Version | Purpose |
|------------|---------|---------|
| Rust | 1.93.0 | HTTP/MCP server runtime |
| Axum | 0.8 | REST and MCP route handling |
| tower-http | 0.6 | CORS, static files, request IDs, and tracing |
| `ugoite-core` crate | Workspace path dependency | Shared application service facade |

### Frontend (TypeScript/SolidStart)

| Technology | Version | Purpose |
|------------|---------|---------|
| Deno | 2.8.3 | TypeScript runtime, workspace tasks, and package resolution |
| SolidJS | Latest | Reactive UI framework |
| SolidStart | Latest | Full-stack framework |
| TailwindCSS | Latest | Styling |

## Development Tools

| Tool | Purpose |
|------|---------|
| cargo | Rust build, run, and test orchestration |
| mise | Task runner and version management |
| rustfmt | Rust formatting |
| clippy | Rust linting |
| deno fmt/lint/check | TypeScript formatting, linting, and static checks |
| vitest | Frontend unit testing |
| Playwright on Deno | E2E testing |

## Storage Backends

OpenDAL provides unified access to multiple storage systems:

| Backend | Use Case | Configuration |
|---------|----------|---------------|
| `fs` | Local development, personal use | `fs:///path/to/data` |
| `memory` | Testing, temporary storage | `memory://` |
| `s3` | Cloud storage (AWS, MinIO) | `s3://bucket/prefix` |
| `gcs` | Google Cloud Storage | `gcs://bucket/prefix` |
| `azblob` | Azure Blob Storage | `azblob://container/prefix` |

## Build Targets

The Rust core layer targets multiple runtimes:

| Layer / Target | Use Case |
|----------------|----------|
| `ugoite-minimum` (native) | Portable domain logic for adapters and future runtimes |
| `ugoite-core` (native) | Server and CLI integration over OpenDAL-backed storage |
| `ugoite-cli` (native) | Native Rust CLI binary using Clap and backend/API routing |
| `ugoite-minimum` (future WebAssembly) | Browser-based frontend and other sandboxed clients |
| Tauri integration | Desktop application |

## CI/CD Pipeline

| Stage | Tools | Trigger |
|-------|-------|---------|
| Lint | clippy, deno lint | Push, PR |
| Type Check | cargo check, deno check | Push, PR |
| Unit Test | cargo test, vitest | Push, PR |
| E2E Test | Playwright on Deno | Push, PR |
| Build | Docker, Cargo | Release |
