# Architecture Decision Records

This document captures key architectural decisions made during IEapp development.

## ADR-001: Rust Core with Python Bindings

**Status**: Accepted (Milestone 2)

**Context**: 
- Core logic was initially implemented in Python using fsspec
- Need to support native desktop apps (Tauri) and WebAssembly
- fsspec is Python-only, not portable to other platforms

**Decision**: 
Extract core logic into a Rust crate (`ieapp-core`) with:
- OpenDAL for storage abstraction (Rust-native fsspec equivalent)
- pyo3 for Python bindings
- wasm-bindgen for WebAssembly (future)

**Consequences**:
- (+) Single source of truth for core logic
- (+) Enables native desktop app without Python runtime
- (+) Better performance for large workspaces
- (-) Additional build complexity
- (-) Learning curve for Rust

---

## ADR-002: Class-based Note Typing

**Status**: Accepted

**Context**: 
- Users need structure without rigid database schemas
- Pure Markdown lacks metadata capabilities
- Traditional forms are cumbersome for knowledge work

**Decision**: 
Use "Classes" to define note types:
- Class definitions stored as JSON in `classes/{name}.json`
- Notes reference a class via frontmatter: `class: Meeting`
- H2 headers (`## Field`) become typed properties
- Live indexer extracts and validates properties

**Consequences**:
- (+) Flexible: users can ignore classes entirely
- (+) Structured: when needed, data is queryable
- (+) Portable: standard Markdown with metadata
- (-) Complexity: indexer must parse Markdown

---

## ADR-003: MCP Resource-First Integration

**Status**: Superseded (Milestone 2)

**Context**:
- The initial plan used a sandboxed code execution tool to reduce tool surface
- Requirements shifted to remove the Wasm sandbox in Milestone 2

**Decision**:
Use MCP resources (and future explicit tools) without sandboxed execution. The
`run_script` tool is deprecated and removed from the backend.

**Consequences**:
- (+) Simpler runtime and dependency footprint
- (+) Clearer security posture without code execution
- (-) Less flexibility until explicit MCP tools are added

---

## ADR-004: Local-First Storage

**Status**: Accepted

**Context**: 
- Users want control over their data
- Cloud storage adds cost and complexity
- Offline access is important for productivity

**Decision**: 
Use fsspec/OpenDAL for storage abstraction:
- Default: local filesystem
- Optional: S3, GCS, Azure Blob
- No required cloud services
- Data format: JSON + Markdown (human-readable)

**Consequences**:
- (+) User owns their data completely
- (+) Works offline
- (+) Multiple storage backends supported
- (-) No built-in sync (user must configure)
- (-) No built-in backup (user responsibility)

---

## ADR-005: Optimistic Concurrency Control

**Status**: Accepted

**Context**: 
- Multiple clients may edit the same note
- Traditional locking is too restrictive
- Users expect responsive UI

**Decision**: 
Use revision-based optimistic concurrency:
- Every note has a `revision_id`
- Updates include `parent_revision_id`
- Server rejects if parent doesn't match current
- Client handles 409 Conflict with merge UI

**Consequences**:
- (+) Responsive UI with optimistic updates
- (+) No data loss (conflicts are surfaced)
- (+) Simple implementation
- (-) Users must resolve conflicts manually
- (-) Last-write-wins would be simpler (but risky)

---

## ADR-006: Backend as Pure API Layer

**Status**: Accepted (Milestone 2)

**Context**: 
- Original backend contained business logic
- Duplicated validation between backend and core logic
- Hard to test backend in isolation

**Decision**: 
Backend should be a pure API layer:
- All business logic in `ieapp-core`
- Backend only routes requests and formats responses
- No direct filesystem access in backend
- Backend tests use memory filesystem via `ieapp-core`

**Consequences**:
- (+) Single source of truth for business logic
- (+) Easier to maintain and test
- (+) Backend becomes simpler
- (-) More abstraction layers
- (-) Slightly more latency

**Note**: `ieapp-cli` is a separate tool for direct command-line interaction with `ieapp-core`.

---

## ADR-007: YAML for Program-Readable Documentation

**Status**: Accepted (Milestone 2)

**Context**: 
- Markdown documentation is hard to parse programmatically
- Need to verify documentation matches code
- Requirements tracking is manual and error-prone

**Decision**: 
Use YAML for machine-readable specifications:
- `features.yaml`: Feature paths across modules
- `requirements/*.yaml`: Requirements with test mapping
- `stories/*.yaml`: User stories with acceptance criteria
- Automated tests verify consistency

**Consequences**:
- (+) Documentation is verifiable
- (+) Requirements traceability is automated
- (+) Easier to generate reports
- (-) More structured format to maintain
- (-) YAML can be verbose
