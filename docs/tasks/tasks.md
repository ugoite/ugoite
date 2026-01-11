# Milestone 2: Full Configuration

**Status**: 🔄 In Progress  
**Goal**: Codebase unification, architecture refinement, and documentation automation

This milestone focuses on improving code quality, consistency, and preparing the architecture for future extensibility (native apps, web assembly deployment).

---

## Overview

### Vision

Transform the MVP codebase into a well-structured, maintainable system with:
- Unified terminology and consistent patterns
- Rust core library for multi-platform deployment
- Automated documentation-to-code verification
- Program-readable specifications

### Key Metrics

| Metric | Target |
|--------|--------|
| Test Coverage | >80% |
| Documentation Consistency | 100% (verified by tests) |
| Feature Path Alignment | All modules match `features.yaml` |
| Requirements Traceability | All tests linked to requirements |

---

## Checkpoint 1: Terminology Unification ✅ DONE (Refactored to "Note Class")

**Goal**: Consolidate "datamodel", "schema", "class" terminology to single "class" term

### AS-IS (Status: Migrated)

The codebase now consistently uses "Class" or "Note Class".

| Location | Term Used | Status |
|----------|-----------|--------|
| Specification | "Note Class" | Updated |
| CLI Logic | `classes.py` | Renamed / Updated |
| API Routes | `/classes` | Updated |
| Models | `ClassCreate` | Updated |
| Frontend | `classApi` | Updated |
| Directory | `classes/` | Migrated |

### TO-BE (Verified)

Unified to "Class" terminology:

| Location | New Term | Changes Required |
|----------|----------|------------------|
| Documentation | "Class" | Update all spec files |
| `ieapp-cli/src/ieapp/classes.py` | "class" | Rename `schemas.py` → `classes.py`, rename functions |
| `backend/src/app/api/endpoints/` | "class" | Routes: `/classes`, `/classes/{name}` |
| `backend/src/app/models/classes.py` | "ClassCreate" | Rename models file |
| `frontend/src/lib/client.ts` | "classApi" | Rename API client methods |
| Directory structure | `classes/` | Rename workspace subdirectory |

### Tasks

- [ ] Update `docs/spec/` to use "Class" consistently
- [ ] Rename `ieapp-cli/src/ieapp/schemas.py` → `classes.py`
- [ ] Rename functions: `list_schemas` → `list_classes`, `get_schema` → `get_class`, etc.
- [ ] Update backend routes and models
- [ ] Update frontend API client
- [ ] Create migration script for existing workspaces (`schemas/` → `classes/`)
- [ ] Update tests to use new terminology
- [ ] Add backwards compatibility layer (optional, for existing users)

### Acceptance Criteria

- [ ] No references to "schema" (lowercase) in user-facing code/docs (except Python `BaseModel` internals)
- [ ] All tests pass with new terminology
- [ ] Existing workspaces can be migrated automatically

---

## Checkpoint 2: Rust Core Library (ieapp-core)

**Goal**: Extract core logic into a Rust crate for multi-platform deployment

### AS-IS (Current State)

```
ieapp-cli/
├── src/ieapp/
│   ├── workspace.py      # Workspace CRUD, fsspec operations
│   ├── notes.py          # Note CRUD, revision history
│   ├── schemas.py        # Class definitions
│   ├── indexer.py        # Structured data extraction
│   ├── attachments.py    # Binary file management
│   ├── links.py          # Note-to-note links
│   ├── integrity.py      # HMAC signing
│   ├── sandbox/          # Wasm sandbox (Python wrapper)
│   └── utils.py          # fsspec helpers
backend/
├── src/app/
│   ├── api/              # FastAPI routes (calls ieapp-core)
│   └── mcp/              # MCP server endpoints
```

**Issues**:
- Core logic is Python-only, not usable from Tauri or WebAssembly
- fsspec is Python-specific, not portable to Rust ecosystem
- Backend duplicates some validation logic

### TO-BE (Target State)

```
ieapp-core/                     # NEW: Rust crate (Mixed Layout)
├── Cargo.toml                  # Rust package config
├── pyproject.toml              # Python package config (maturin)
├── ieapp_core/                 # Python package
│   └── __init__.py
├── src/                        # Rust core + bindings
│   ├── lib.rs                  # pyo3 entry point & bindings
│   ├── workspace.rs           # Workspace operations
│   ├── note.rs                # Note operations
│   ├── class.rs               # Class definitions
│   ├── index.rs               # Indexing logic
│   ├── attachment.rs          # Attachment handling
│   ├── link.rs                # Note links
│   ├── integrity.rs           # HMAC/checksum
│   ├── storage/               # Storage abstraction
│   │   ├── mod.rs
│   │   ├── opendal.rs         # OpenDAL backend
│   │   └── memory.rs          # In-memory for tests
│   └── sandbox/               # Wasm sandbox (wasmtime)
│       └── mod.rs

ieapp-cli/                      # UPDATED: CLI for power users
├── src/ieapp/
│   ├── cli.py                 # Typer-based CLI
│   └── compat.py              # Compatibility layer (optional)

backend/                        # UPDATED: Pure API layer (calls ieapp-core)
├── src/app/
│   ├── api/                   # Routes (delegate to ieapp-core)
│   └── mcp/                   # MCP server (delegate to ieapp-core)

frontend/                       # UNCHANGED: UI only
```

### Responsibility Matrix (TO-BE)

| Module | Responsibility |
|--------|----------------|
| `ieapp-core` (Rust) | All data operations, storage abstraction (OpenDAL), validation, indexing, sandbox |
| `ieapp-cli` (Python) | Typer CLI for direct user interaction |
| `backend` (Python) | REST API routes, MCP server, delegates to ieapp-core |
| `frontend` (TypeScript) | UI rendering, optimistic updates, no data logic |

### Tasks

- [ ] Create `ieapp-core/` Rust crate with Cargo.toml
- [ ] Implement storage abstraction using OpenDAL
- [ ] Port `workspace.py` → `workspace.rs`
- [ ] Port `notes.py` → `note.rs`
- [ ] Port `classes.py` (was schemas.py) → `class.rs`
- [ ] Port `indexer.py` → `index.rs`
- [ ] Port `attachments.py` → `attachment.rs`
- [ ] Port `integrity.py` → `integrity.rs`
- [ ] Port Wasm sandbox to Rust (wasmtime native)
- [ ] Create pyo3 Python bindings
- [ ] Update ieapp-cli to use Rust bindings
- [ ] Update backend to use ieapp-core bindings (no direct file access)
- [ ] Ensure all tests pass with new architecture
- [ ] Benchmark performance vs Python implementation

### Technology Choices

| Component | Library | Rationale |
|-----------|---------|-----------|
| Storage | [OpenDAL](https://opendal.apache.org/) | Rust-native fsspec equivalent, supports S3/GCS/local/memory |
| Python Bindings | [pyo3](https://pyo3.rs/) | De facto standard for Rust-Python FFI |
| Wasm Sandbox | [wasmtime](https://wasmtime.dev/) | Already used, native Rust integration |
| JSON | serde_json | Standard Rust serialization |

### Acceptance Criteria

- [ ] `ieapp-core` compiles to native library and Wasm
- [ ] Python bindings work with existing ieapp-cli tests
- [ ] Backend has zero direct filesystem operations
- [ ] Performance is equal or better than Python implementation

---

## Checkpoint 3: Feature Path Consistency

**Goal**: Standardize directory structure and paths across all modules

### AS-IS (Current State)

Path patterns are inconsistent across modules:

| Feature | ieapp-cli | backend | frontend |
|---------|-----------|---------|----------|
| Workspace | `workspace.py` | `api/endpoints/workspaces.py` | `workspace-store.ts` |
| Notes | `notes.py` | `api/endpoints/workspaces.py` (mixed) | `store.ts`, `routes/notes/` |
| Classes | `schemas.py` | `api/endpoints/workspaces.py` (mixed) | `client.ts` (schemaApi) |
| Attachments | `attachments.py` | `api/endpoints/workspaces.py` | (in store) |
| Search | `search.py` | `api/endpoints/workspaces.py` | `client.ts` |

### TO-BE (Target State)

All modules follow the same feature-based structure:

```yaml
# docs/spec/features.yaml
features:
  workspace:
    crate: src/workspace.rs
    ieapp_core: src/ieapp/workspace.py
    backend: src/app/api/endpoints/workspace.py
    frontend: src/lib/workspace-store.ts
    
  note:
    crate: src/note.rs
    ieapp_core: src/ieapp/note.py
    backend: src/app/api/endpoints/note.py
    frontend: src/lib/note-store.ts
    
  class:
    crate: src/class.rs
    ieapp_core: src/ieapp/class.py
    backend: src/app/api/endpoints/class.py
    frontend: src/lib/class-store.ts
    
  attachment:
    crate: src/attachment.rs
    ieapp_cli: src/ieapp/attachment.py
    backend: src/app/api/endpoints/attachment.py
    frontend: src/lib/attachment-store.ts
    
  link:
    crate: src/link.rs
    ieapp_cli: src/ieapp/link.py
    backend: src/app/api/endpoints/link.py
    frontend: src/lib/link-store.ts
    
  search:
    crate: src/search.rs
    ieapp_cli: src/ieapp/search.py
    backend: src/app/api/endpoints/search.py
    frontend: src/lib/search-store.ts
    
  index:
    crate: src/index.rs
    ieapp_cli: src/ieapp/index.py
    backend: (internal, no API)
    frontend: (not applicable)
    
  sandbox:
    crate: src/sandbox/
    ieapp_cli: src/ieapp/sandbox/
    backend: src/app/mcp/sandbox.py
    frontend: (not applicable)
```

### Tasks

- [x] Define API-level feature registry under `docs/spec/features/`
- [ ] Refactor backend: split `workspaces.py` into feature-specific files
- [ ] Refactor frontend: organize stores by feature
- [ ] Rename `schemas.py` → `class.py` (part of Checkpoint 1)
- [ ] Create path verification test for each module
- [ ] Add CI check for feature path consistency

### Acceptance Criteria

- [x] `docs/spec/features/` defines API operations with paths + symbol locations
- [ ] All modules follow the defined path structure
- [ ] Automated tests verify path consistency

---

## Checkpoint 4: Requirements Automation

**Goal**: YAML-based requirements with automated test verification

### AS-IS (Current State)

- Requirements in `docs/spec/requirements/*.yaml` (machine-readable)
- Manual mapping of tests to requirements
- No automated verification that tests exist for requirements
- No verification that tests reference valid requirements

### TO-BE (Target State)

```
docs/spec/requirements/
├── README.md              # Explanation of requirements format
├── storage.yaml           # REQ-STO-* requirements
├── note.yaml              # REQ-NOTE-* requirements
├── index.yaml             # REQ-IDX-* requirements
├── integrity.yaml         # REQ-INT-* requirements
├── security.yaml          # REQ-SEC-* requirements
├── sandbox.yaml           # REQ-SANDBOX-* requirements
├── api.yaml               # REQ-API-* requirements
├── frontend.yaml          # REQ-FE-* requirements
├── class.yaml              # REQ-CLS-* requirements
└── e2e.yaml               # REQ-E2E-* requirements

docs/tests/
├── README.md              # Test framework documentation
├── test_requirements.py   # Verify requirements coverage
├── test_features.py       # Verify feature path consistency
└── test_docs_links.py     # Verify documentation cross-references
```

### Requirements YAML Format

```yaml
# docs/spec/requirements/storage.yaml
category: Storage & Data Model
prefix: REQ-STO

requirements:
  - id: REQ-STO-001
    title: Storage Abstraction Using fsspec
    description: System MUST use fsspec for all I/O operations.
    related_spec:
      - architecture/overview.md#high-level-architecture
      - stories/core.yaml
    tests:
      pytest:
        - file: ieapp-cli/tests/test_workspace.py
          tests:
            - test_create_workspace_scaffolding
            - test_create_workspace_s3_unimplemented
        - file: ieapp-cli/tests/test_indexer.py
          tests:
            - test_indexer_run_once
            
  - id: REQ-STO-002
    title: Workspace Directory Structure
    # ...
```

### Verification Tests

```python
# docs/tests/test_requirements.py
def test_all_requirements_have_tests():
    """Every requirement must have at least one associated test."""
    
def test_all_tests_reference_valid_requirements():
    """Tests that claim to verify requirements must reference valid REQ-* IDs."""
    
def test_no_orphan_tests():
    """Tests without requirement references are flagged for review."""
```

### Tasks

- [x] Create `docs/spec/requirements/` directory structure
- [x] Convert legacy requirements to YAML format
- [x] Create `docs/spec/requirements/README.md` with format explanation
- [x] Create `docs/tests/README.md` with test framework documentation
- [ ] Implement `test_requirements.py` (requirement coverage verification)
- [ ] Implement `test_features.py` (feature path verification)
- [ ] Add test name convention: `test_<feature>_<requirement_id>_<description>`
- [ ] Update CI to run document verification tests

### Acceptance Criteria

- [x] All requirements converted to YAML format
- [ ] Document tests verify 100% requirement coverage
- [ ] Orphan tests (no requirement) are identified and reviewed
- [ ] CI fails if requirements are not covered

---

## Checkpoint 5: Documentation Restructure

**Goal**: Program-readable specifications with automated consistency checks

### AS-IS (Current State)

Legacy numbered spec files were used during MVP development.
They have been migrated into the v2 structure and removed.

**Issues**:
- Numbered prefixes don't indicate content
- Mixed concerns (stories + features + functional requirements)
- User stories in Markdown, not easily parseable

### TO-BE (Target State)

```
docs/spec/
├── index.md                      # Master index with navigation
├── README.md                     # Entry point + conventions
├── architecture/
│   ├── overview.md               # High-level architecture
│   ├── stack.md                  # Technology stack details
│   └── decisions.md              # Architecture Decision Records
├── features/
│   ├── features.yaml             # Registry manifest
│   ├── *.yaml                    # API-level registries by kind
│   └── README.md                 # Feature documentation
├── stories/
│   ├── README.md                 # Story format explanation
│   ├── core.yaml                 # Core user stories (Story 1-5)
│   ├── advanced.yaml             # Advanced features (Story 6-9)
│   └── experimental.yaml         # Future features
├── data-model/
│   ├── overview.md               # Data model explanation
│   ├── file-schemas.yaml         # JSON schema definitions
│   └── directory-structure.md    # Workspace layout
├── api/
│   ├── rest.md                   # REST API documentation
│   ├── mcp.md                    # MCP protocol documentation
│   └── openapi.yaml              # OpenAPI specification
├── requirements/
│   ├── README.md                 # Requirements format
│   ├── storage.yaml
│   ├── note.yaml
│   └── ... (per category)
├── security/
│   ├── overview.md               # Security strategy
│   ├── sandbox.md                # Wasm sandbox details
│   └── authentication.md         # Auth (future)
├── quality/
│   └── error-handling.md          # Error handling & resilience
└── product/
  └── success-metrics.md         # Success metrics
└── testing/
    ├── strategy.md               # Testing approach
    └── ci-cd.md                  # CI/CD documentation

docs/tests/
├── README.md                     # Test framework documentation
├── conftest.py                   # Shared test fixtures
├── test_requirements.py          # Requirement coverage
├── test_features.py              # Feature path consistency
├── test_stories.py               # Story format validation
└── test_docs_links.py            # Cross-reference verification
```

### User Stories YAML Format

```yaml
# docs/spec/stories/core.yaml
category: Core User Stories
stories:
  - id: STORY-001
    title: The Programmable Knowledge Base
    type: AI Native
    as_a: power user or AI agent
    i_want: to execute JavaScript code against my notes
    so_that: I can perform complex tasks like analyzing data and generating reports
    acceptance_criteria:
      - MCP Tool run_script is available
      - AI can call host functions via host.call()
      - AI can query structured properties via API
      - Output (text/charts) is returned to AI context
    related_apis:
      - MCP tool: run_script
      - REST: POST /workspaces/{ws_id}/query
    requirements:
      - REQ-SANDBOX-001
      - REQ-SANDBOX-002
```

### Tasks

- [x] Create new directory structure under `docs/spec/`
- [x] Migrate legacy spec content into v2 directories
- [x] Convert requirements into `docs/spec/requirements/*.yaml`
- [x] Create `docs/spec/index.md` with navigation
- [x] Create `docs/spec/README.md` entry point
- [x] Create `docs/tests/README.md` explaining test framework
- [x] Update/clean up cross-references in spec

### Acceptance Criteria

- [ ] All content migrated to new structure
- [ ] YAML files are valid and parseable
- [ ] Cross-references are updated and verified
- [ ] `docs/tests/README.md` documents the test framework

---

## Timeline (Tentative)

```
Week 1-2  |████| Checkpoint 1: Terminology Unification
Week 3-6  |████████████████| Checkpoint 2: Rust Core Library
Week 7-8  |████| Checkpoint 3: Feature Path Consistency
Week 9-10 |████| Checkpoint 4: Requirements Automation
Week 11-12|████| Checkpoint 5: Documentation Restructure
```

---

## Definition of Done

For this milestone to be considered complete:

- [ ] All checkpoints completed with acceptance criteria met
- [ ] All tests pass (unit, integration, e2e)
- [ ] Documentation updated and consistent
- [ ] CI/CD includes document verification tests
- [ ] No regressions in existing functionality
- [ ] README.md updated to reflect new structure

---

## References

- [Roadmap](roadmap.md) - Future milestones
- [MVP Archive](archive/mvp-milestone.md) - Previous milestone
- [Architecture Overview](../spec/architecture/overview.md) - System design (TO-BE)
