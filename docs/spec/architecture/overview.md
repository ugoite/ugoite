# Architecture Overview

## 1. High-Level Architecture

IEapp follows a **Local-First, Server-Relay** architecture. The system is designed for:

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
| `note.rs` | Note CRUD via Iceberg tables, revision history, conflict detection |
| `class.rs` | Iceberg class schema management |
| `index.rs` | Structured data extraction, derived indexes |
| `attachment.rs` | Binary file storage, deduplication |
| `link.rs` | Note-to-note relationships |
| `integrity.rs` | HMAC signing, checksum verification |
| `search.rs` | Full-text and structured queries |

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

IEapp bridges the gap between Markdown freedom and database structure:

1. **Parse**: Scan Markdown for H2 headers (`## Key`)
2. **Extract**: Convert headers + content to structured properties
3. **Validate**: Check against Class definition (if assigned)
4. **Index**: Update derived indexes for fast queries

This enables "Markdown sections as database fields" without complex forms.

## 4. Data Flow Example

**Creating a Note:**

```
Frontend                 Backend              ieapp-core           Storage
   │                        │                     │                   │
   │ POST /notes            │                     │                   │
   │───────────────────────>│                     │                   │
   │                        │ create_note()       │                   │
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
| **Layered** | Clear separation: Core → {CLI, Backend} → Frontend |
| **Testable** | Each layer independently testable; memory storage for fast tests |
