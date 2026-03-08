# Architecture Overview

## 1. High-Level Architecture

Ugoite follows a **Local-First, Server-Relay** architecture. The system is designed for:

- **Portability**: Iceberg tables (Parquet) with Markdown reconstruction
- **AI Integration**: First-class support for AI agents via MCP
- **Multi-Platform**: Core logic in Rust enables native apps and WebAssembly

```
┌─────────────────────────────┐   ┌─────────────────────────────┐
│            User             │   │           Browser            │
└──────────────┬──────────────┘   └──────────────┬──────────────┘
               │                                 │
     ┌─────────┴─────────┐             ┌─────────┴─────────┐
     │  Frontend App     │             │     Terminal      │
     │  (Web/Desktop)    │             │     (Power User)  │
     └─────────┬─────────┘             └─────────┬─────────┘
               │                                 │
               ▼                                 ▼
┌─────────────────────────────┐   ┌─────────────────────────────┐
│     Backend (FastAPI)       │   │      ugoite-cli (Python)     │
│  - REST API & MCP Server    │   │  - Typer-based CLI          │
│  - Auth & Orchestration     │   │  - Direct data access       │
└──────────────┬──────────────┘   └──────────────┬──────────────┘
               │                                 │
               └────────────────┬────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│               ugoite-core (Rust adapter crate)                  │
│   - OpenDAL-backed storage adapter                              │
│   - Iceberg/Parquet integrations + Python bindings             │
├─────────────────────────────────────────────────────────────────┤
│               ugoite-minimum (Rust portable crate)              │
│   - Portable domain logic + storage abstraction traits          │
│   - Foundation for native and future WebAssembly targets        │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│              Storage Layer (OpenDAL-backed adapters)            │
│   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐        │
│   │  Local  │   │   S3    │   │  GCS    │   │ Memory  │        │
│   │  Disk   │   │ / MinIO │   │         │   │ (test)  │        │
│   └─────────┘   └─────────┘   └─────────┘   └─────────┘        │
└─────────────────────────────────────────────────────────────────┘
```

## 2. Module Responsibilities

### ugoite-minimum (Rust)

The portable core library owns runtime-neutral abstractions and shared models:

| Component | Responsibility |
|-----------|----------------|
| `storage.rs` | Runtime-neutral storage traits and JSON helpers |
| `space.rs` | Portable space metadata and URI normalization |
| `integrity.rs` | Integrity traits and cryptographic primitives |
| `link.rs`, `search.rs`, `metadata.rs` | Shared domain models and metadata rules |

### ugoite-core (Rust)

The adapter crate depends on `ugoite-minimum` and keeps the heavier integrations:

| Component | Responsibility |
|-----------|----------------|
| `storage/` | OpenDAL adapter implementation for `ugoite-minimum::storage` |
| `space.rs` | Space CRUD, directory scaffolding |
| `entry.rs` | Entry CRUD via Iceberg tables, revision history, conflict detection |
| `form.rs` | Iceberg form schema management |
| `index.rs` | Structured data extraction, derived indexes |
| `asset.rs` | Binary file storage, deduplication |
| `link.rs` | Entry-to-entry relationships |
| `integrity.rs` | HMAC signing, checksum verification |
| `search.rs` | Full-text and structured queries |

### ugoite-cli (Python)

Command-line interface for power users:

| Component | Responsibility |
|-----------|----------------|
| `cli.py` | Typer-based CLI |
| `compat.py` | Backwards compatibility helpers |

### Backend (Python/FastAPI)

API layer providing access to frontend and AI agents:

| Component | Responsibility |
|-----------|----------------|
| `api/endpoints/` | REST route handlers (call ugoite-core bindings) |
| `mcp/` | MCP protocol implementation |
| `models/` | Pydantic request/response models |
| `core/` | Configuration, middleware |

### Frontend (TypeScript/SolidStart)

UI layer with NO data logic:

| Component | Responsibility |
|-----------|----------------|
| `lib/*-store.ts` | State management, optimistic updates |
| `lib/*-api.ts` | Feature API clients (REST calls only) |
| `routes/` | Page components |
| `components/` | Reusable UI components |

## 3. The "Structure-from-Text" Engine

Ugoite bridges the gap between Markdown freedom and database structure:

1. **Parse**: Scan Markdown for H2 headers (`## Key`)
2. **Extract**: Convert headers + content to structured properties
3. **Validate**: Check against Form definition (if assigned)
4. **Index**: Update derived indexes for fast queries

This enables "Markdown sections as database fields" without complex forms.

## 4. Data Flow Example

**Creating an Entry:**

```
Frontend                 Backend              ugoite-core           Storage
   │                        │                     │                   │
   │ POST /spaces/:id/entries│                    │                   │
   │───────────────────────>│                     │                   │
   │                        │ create_entry()      │                   │
   │                        │────────────────────>│                   │
   │                        │                     │ write Iceberg rows│
   │                        │                     │──────────────────>│
   │                        │                     │ update indexes    │
   │                        │                     │──────────────────>│
   │                        │                     │<──────────────────│
   │                        │<────────────────────│                   │
   │ 201 Created            │                     │                   │
   │<───────────────────────│                     │                   │
   │                        │                     │                   │
   │ (optimistic update     │                     │                   │
   │  already rendered)     │                     │                   │
```

## 5. Design Principles

| Principle | Implementation |
|-----------|----------------|
| **Local-First** | All data in user-controlled storage; no required cloud services |
| **Portable** | Iceberg tables (Parquet) + Markdown reconstruction; easy export/import |
| **AI-Native** | MCP protocol + MCP integration for AI agents |
| **Layered** | Clear separation: ugoite-minimum → ugoite-core → {CLI, Backend} → Frontend |
| **Testable** | Each layer independently testable; memory storage for fast tests |
