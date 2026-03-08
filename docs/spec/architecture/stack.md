# Technology Stack

## Overview

Ugoite uses a modern stack optimized for local-first operation and AI integration.

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
| [pyo3](https://pyo3.rs/) | Latest | Python bindings |
| [wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/) | Latest | WebAssembly bindings (future) |

### ugoite-cli (Python)

| Technology | Version | Purpose |
|------------|---------|---------|
| Python | 3.12+ | Runtime |
| Typer | Latest | CLI framework |
| pyo3 bindings | - | Rust core integration |

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
| mise | Task runner and version management |
| uv | Python package management |
| ruff | Python linting and formatting |
| ty | Python type checking |
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

The Rust core layer targets multiple runtimes:

| Layer / Target | Use Case |
|----------------|----------|
| `ugoite-minimum` (native) | Portable domain logic for adapters and future runtimes |
| `ugoite-core` (native + Python bindings) | backend & ugoite-cli via pyo3 |
| `ugoite-minimum` (future WebAssembly) | Browser-based frontend and other sandboxed clients |
| Tauri integration | Desktop application |

## CI/CD Pipeline

| Stage | Tools | Trigger |
|-------|-------|---------|
| Lint | ruff, biome | Push, PR |
| Type Check | ty, TypeScript | Push, PR |
| Unit Test | pytest, vitest | Push, PR |
| E2E Test | bun:test | Push, PR |
| Build | Docker, Cargo | Release |
