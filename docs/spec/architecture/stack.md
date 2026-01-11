# Technology Stack

## Overview

IEapp uses a modern stack optimized for local-first operation and AI integration.

## Core Technologies

### ieapp-core (Rust Crate)

| Technology | Version | Purpose |
|------------|---------|---------|
| Rust | 1.75+ | Core language |
| [OpenDAL](https://opendal.apache.org/) | Latest | Storage abstraction (local, S3, GCS, memory) |
| [wasmtime](https://wasmtime.dev/) | Latest | WebAssembly runtime for sandbox |
| [serde](https://serde.rs/) | Latest | JSON serialization |
| [pyo3](https://pyo3.rs/) | Latest | Python bindings |
| [wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/) | Latest | WebAssembly bindings (future) |

### ieapp-cli (Python)

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
| `fs` | Local development, personal use | `file:///path/to/data` |
| `memory` | Testing, temporary storage | `memory://` |
| `s3` | Cloud storage (AWS, MinIO) | `s3://bucket/prefix` |
| `gcs` | Google Cloud Storage | `gcs://bucket/prefix` |
| `azblob` | Azure Blob Storage | `azblob://container/prefix` |

## Build Targets

The ieapp-core crate compiles to multiple targets:

| Target | Use Case |
|--------|----------|
| Native (x86_64, arm64) | backend & ieapp-cli via pyo3 |
| WebAssembly | Browser-based frontend and sandbox |
| Tauri integration | Desktop application |

## CI/CD Pipeline

| Stage | Tools | Trigger |
|-------|-------|---------|
| Lint | ruff, biome | Push, PR |
| Type Check | ty, TypeScript | Push, PR |
| Unit Test | pytest, vitest | Push, PR |
| E2E Test | bun:test | Push, PR |
| Build | Docker, Cargo | Release |
