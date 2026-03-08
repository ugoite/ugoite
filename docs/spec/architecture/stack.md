# Technology Stack

## Overview

Ugoite uses a modern stack optimized for local-first operation and AI integration.

## Core Technologies

### ugoite-core (Rust Crate)

| Technology | Version | Purpose |
|------------|---------|---------|
| Rust | 1.75+ | Core language |
| [OpenDAL](https://opendal.apache.org/) | Latest | Storage abstraction (local, S3, GCS, memory) |
| [serde](https://serde.rs/) | Latest | JSON serialization |
| [pyo3](https://pyo3.rs/) | Latest | Python bindings |
| [wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/) | Latest | WebAssembly bindings (future) |

### ugoite-cli (Rust)

| Technology | Version | Purpose |
|------------|---------|---------|
| Rust | 1.75+ | CLI runtime |
| [clap](https://docs.rs/clap/) | Latest | Command parsing and help output |
| [reqwest](https://docs.rs/reqwest/) | Latest | Backend/API routing |
| `ugoite-core` crate | Workspace path dependency | Shared core integration |

### Backend (Python/FastAPI)

| Technology | Version | Purpose |
|------------|---------|---------|
| Python | 3.12+ | Runtime |
| FastAPI | Latest | Web framework |
| uvicorn | Latest | ASGI server |
| Pydantic | v2 | Request/response validation |

### Frontend (TypeScript/SolidStart)

| Technology | Version | Purpose |
|------------|---------|---------|
| Bun | Latest | JavaScript runtime & package manager |
| SolidJS | Latest | Reactive UI framework |
| SolidStart | Latest | Full-stack framework |
| TailwindCSS | Latest | Styling |

## Development Tools

| Tool | Purpose |
|------|---------|
| cargo | Rust build, run, and test orchestration |
| mise | Task runner and version management |
| uv | Python package management |
| ruff | Python linting and formatting |
| ty | Python type checking |
| rustfmt | Rust formatting |
| clippy | Rust linting |
| biome | TypeScript/JavaScript linting |
| pytest | Python testing |
| vitest | Frontend unit testing |
| bun:test | E2E testing |

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

The ugoite-core crate compiles to multiple targets:

| Target | Use Case |
|--------|----------|
| Native (x86_64, arm64) | ugoite-cli native binary; backend via Python bindings |
| WebAssembly | Browser-based frontend |
| Tauri integration | Desktop application |

## CI/CD Pipeline

| Stage | Tools | Trigger |
|-------|-------|---------|
| Lint | ruff, biome | Push, PR |
| Type Check | ty, TypeScript | Push, PR |
| Unit Test | pytest, vitest | Push, PR |
| E2E Test | bun:test | Push, PR |
| Build | Docker, Cargo | Release |
