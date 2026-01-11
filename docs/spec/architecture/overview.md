# Architecture Overview

## 1. High-Level Architecture

IEapp follows a **Local-First, Server-Relay** architecture. The system is designed for:

- **Portability**: Data stored in standard formats (JSON, Markdown)
- **AI Integration**: First-class support for AI agents via MCP
- **Multi-Platform**: Core logic in Rust enables native apps and WebAssembly

```
┌─────────────────────────────┐   ┌─────────────────────────────┐
│                         User / Browser                          │
└──────────────┬──────────────┘   └──────────────┬──────────────┘
               │                                 │
     ┌─────────┴─────────┐             ┌─────────┴─────────┐
     │  Frontend App     │             │     Terminal      │
     │  (Web/Desktop)    │             │     (Power User)  │
     └─────────┬─────────┘             └─────────┬─────────┘
               │                                 │
               ▼                                 ▼
┌─────────────────────────────┐   ┌─────────────────────────────┐
│     Backend (FastAPI)       │   │      ieapp-cli (Python)     │
│  - REST API & MCP Server    │   │  - Typer-based CLI          │
│  - Auth & Orchestration     │   │  - Direct data access       │
└──────────────┬──────────────┘   └──────────────┬──────────────┘
               │                                 │
               └────────────────┬────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                   ieapp-core (Rust Crate)                       │
│   - All data operations                                         │
│   - Storage abstraction (OpenDAL)                               │
│   - Wasm sandbox (wasmtime)                                     │
│   - Compiles to: native, Python binding, WebAssembly            │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Storage Layer (OpenDAL)                       │
│   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐        │
│   │  Local  │   │   S3    │   │  GCS    │   │ Memory  │        │
│   │  Disk   │   │ / MinIO │   │         │   │ (test)  │        │
│   └─────────┘   └─────────┘   └─────────┘   └─────────┘        │
└─────────────────────────────────────────────────────────────────┘
```

## 2. Module Responsibilities

### ieapp-core (Rust)

The core library handles ALL data operations:

| Component | Responsibility |
|-----------|----------------|
| `workspace.rs` | Workspace CRUD, directory scaffolding |
| `note.rs` | Note CRUD, revision history, conflict detection |
| `class.rs` | Class definitions, template generation |
| `index.rs` | Structured data extraction, inverted index |
| `attachment.rs` | Binary file storage, deduplication |
| `link.rs` | Note-to-note relationships |
| `integrity.rs` | HMAC signing, checksum verification |
| `search.rs` | Full-text and structured queries |
| `sandbox/` | Wasm JavaScript execution environment |

### ieapp-cli (Python)

Command-line interface for power users:

| Component | Responsibility |
|-----------|----------------|
| `cli.py` | Typer-based CLI |
| `compat.py` | Backwards compatibility helpers |

### Backend (Python/FastAPI)

API layer providing access to frontend and AI agents:

| Component | Responsibility |
|-----------|----------------|
| `api/endpoints/` | REST route handlers (call ieapp-core bindings) |
| `mcp/` | MCP protocol implementation |
| `models/` | Pydantic request/response schemas |
| `core/` | Configuration, middleware |

### Frontend (TypeScript/SolidStart)

UI layer with NO data logic:

| Component | Responsibility |
|-----------|----------------|
| `lib/*-store.ts` | State management, optimistic updates |
| `lib/client.ts` | API client (REST calls only) |
| `routes/` | Page components |
| `components/` | Reusable UI components |

## 3. The "Code Execution" Paradigm

Instead of building hundreds of specific tools, IEapp exposes a **Wasm Execution Sandbox** to AI agents:

- **Concept**: AI agents write and run JavaScript code to interact with the knowledge base
- **Mechanism**: JavaScript runs in a WebAssembly sandbox (wasmtime + QuickJS)
- **API Access**: Scripts call `host.call(method, path, body)` to access REST endpoints
- **Security**: Strict isolation - no network or filesystem access except via `host.call`

## 4. The "Structure-from-Text" Engine

IEapp bridges the gap between Markdown freedom and database structure:

1. **Parse**: Scan Markdown for H2 headers (`## Key`)
2. **Extract**: Convert headers + content to structured properties
3. **Validate**: Check against Class definition (if assigned)
4. **Index**: Update `index/index.json` for fast queries

This enables "Markdown sections as database fields" without complex forms.

## 5. Data Flow Example

**Creating a Note:**

```
Frontend                 Backend              ieapp-core           Storage
   │                        │                     │                   │
   │ POST /notes            │                     │                   │
   │───────────────────────>│                     │                   │
   │                        │ create_note()       │                   │
   │                        │────────────────────>│                   │
   │                        │                     │ write JSON files  │
   │                        │                     │──────────────────>│
   │                        │                     │ update index      │
   │                        │                     │──────────────────>│
   │                        │                     │<──────────────────│
   │                        │<────────────────────│                   │
   │ 201 Created            │                     │                   │
   │<───────────────────────│                     │                   │
   │                        │                     │                   │
   │ (optimistic update     │                     │                   │
   │  already rendered)     │                     │                   │
```

## 6. Design Principles

| Principle | Implementation |
|-----------|----------------|
| **Local-First** | All data in user-controlled storage; no required cloud services |
| **Portable** | Standard formats (JSON, Markdown); easy export/import |
| **AI-Native** | MCP protocol + code execution sandbox for AI agents |
| **Layered** | Clear separation: Core → {CLI, Backend} → Frontend |
| **Testable** | Each layer independently testable; memory storage for fast tests |
