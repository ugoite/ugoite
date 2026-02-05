# Milestone 2: Full Configuration

**Status**: ✅ Completed  
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

## Checkpoint 1: Terminology Unification ✅ DONE (Refactored to "Entry Form")

**Goal**: Consolidate "datamodel", "schema", "form" terminology to single "form" term

### AS-IS (Status: Migrated)

The codebase now consistently uses "Form" or "Entry Form".

| Location | Term Used | Status |
|----------|-----------|--------|
| Specification | "Entry Form" | Updated |
| CLI Logic | `forms.py` | Renamed / Updated |
| API Routes | `/forms` | Updated |
| Models | `FormCreate` | Updated |
| Frontend | `formApi` | Updated |
| Directory | `forms/` | Migrated |

### TO-BE (Verified)

Unified to "Form" terminology:

| Location | New Term | Changes Required |
|----------|----------|------------------|
| Documentation | "Form" | Update all spec files |
| `ieapp-cli/src/ieapp/forms.py` | "form" | Rename `schemas.py` → `forms.py`, rename functions |
| `backend/src/app/api/endpoints/` | "form" | Routes: `/forms`, `/forms/{name}` |
| `backend/src/app/models/forms.py` | "FormCreate" | Rename models file |
| `frontend/src/lib/*-api.ts` | "formApi" | Rename API client methods |
| Directory structure | `forms/` | Rename space subdirectory |

### Tasks

- [x] Update `docs/spec/` to use "Form" consistently
- [x] Rename `ieapp-cli/src/ieapp/schemas.py` → `forms.py`
- [x] Rename functions: `list_schemas` → `list_forms`, `get_schema` → `get_form`, etc.
- [x] Update backend routes and models
- [x] Update frontend API client
- [x] Create migration script for existing spaces (`schemas/` → `forms/`)
- [x] Update tests to use new terminology
- [x] Add backwards compatibility layer (optional, for existing users)

### Acceptance Criteria

- [x] No references to "schema" (lowercase) in user-facing code/docs (except Python `BaseModel` internals)
- [x] All tests pass with new terminology
- [x] Existing spaces can be migrated automatically

---

## Checkpoint 2: Rust Core Library (ieapp-core) ✅ DONE

**Goal**: Extract core logic into a Rust crate for multi-platform deployment

### AS-IS (Current State)

```
ieapp-cli/
├── src/ieapp/
│   ├── space.py          # Space CRUD, fsspec operations
│   ├── entries.py        # Entry CRUD, revision history
│   ├── forms.py          # Form definitions
│   ├── indexer.py        # Structured data extraction
│   ├── assets.py         # Binary file management
│   ├── links.py          # Entry-to-entry links
│   ├── integrity.py      # HMAC signing
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
│   ├── space.rs               # Space operations
│   ├── entry.rs               # Entry operations
│   ├── form.rs                # Form definitions
│   ├── index.rs               # Indexing logic
│   ├── asset.rs               # Asset handling
│   ├── link.rs                # Entry links
│   ├── integrity.rs           # HMAC/checksum
│   ├── storage/               # Storage abstraction
│   │   ├── mod.rs
│   │   ├── opendal.rs         # OpenDAL backend
│   │   └── memory.rs          # In-memory for tests
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
| `ieapp-core` (Rust) | All data operations, storage abstraction (OpenDAL), validation, indexing |
| `ieapp-cli` (Python) | Typer CLI for direct user interaction |
| `backend` (Python) | REST API routes, MCP server, delegates to ieapp-core |
| `frontend` (TypeScript) | UI rendering, optimistic updates, no data logic |

### Tasks

- [x] Create `ieapp-core/` Rust crate with Cargo.toml
- [x] Implement storage abstraction using OpenDAL
- [x] Port `space.py` → `space.rs`
- [x] Port `entries.py` → `entry.rs`
- [x] Port `forms.py` (was schemas.py) → `form.rs`
- [x] Port `indexer.py` → `index.rs`
- [x] Port `assets.py` → `asset.rs`
- [x] Port `integrity.py` → `integrity.rs`
- [x] Create pyo3 Python bindings
- [x] Update ieapp-cli to use Rust bindings
- [x] Update backend to use ieapp-core bindings (no direct file access)
- [x] Ensure all tests pass with new architecture
- [x] Benchmark performance vs Python implementation

### Technology Choices

| Component | Library | Rationale |
|-----------|---------|-----------|
| Storage | [OpenDAL](https://opendal.apache.org/) | Rust-native fsspec equivalent, supports S3/GCS/local/memory |
| Python Bindings | [pyo3](https://pyo3.rs/) | De facto standard for Rust-Python FFI |
| JSON | serde_json | Standard Rust serialization |

### Acceptance Criteria

- [x] `ieapp-core` compiles to native library and Wasm
- [x] Python bindings work with existing ieapp-cli tests
- [x] Backend has zero direct filesystem operations
- [x] Performance is equal or better than Python implementation

---

## Checkpoint 3: Feature Path Consistency

**Goal**: Standardize directory structure and paths across all modules

### AS-IS (Current State)

Path patterns are inconsistent across modules:

| Feature | ieapp-cli | backend | frontend |
|---------|-----------|---------|----------|
| Space | `space.py` | `api/endpoints/spaces.py` | `space-store.ts` |
| Entries | `entries.py` | `api/endpoints/spaces.py` (mixed) | `store.ts`, `routes/spaces/[space_id]/entries` |
| Forms | `forms.py` | `api/endpoints/spaces.py` (mixed) | `form-api.ts` (formApi) |
| Assets | `assets.py` | `api/endpoints/spaces.py` | (in store) |
| Search | `search.py` | `api/endpoints/spaces.py` | `search-api.ts` |

### TO-BE (Target State)

All modules follow the same feature-based structure:

```yaml
# docs/spec/features.yaml
features:
  space:
    crate: src/space.rs
    ieapp_core: src/ieapp/space.py
    backend: src/app/api/endpoints/space.py
    frontend: src/lib/space-store.ts
    
  entry:
    crate: src/entry.rs
    ieapp_core: src/ieapp/entries.py
    backend: src/app/api/endpoints/entry.py
    frontend: src/lib/entry-store.ts
    
  form:
    crate: src/form.rs
    ieapp_core: src/ieapp/forms.py
    backend: src/app/api/endpoints/forms.py
    frontend: src/lib/form-store.ts
    
  asset:
    crate: src/asset.rs
    ieapp_cli: src/ieapp/assets.py
    backend: src/app/api/endpoints/asset.py
    frontend: src/lib/asset-store.ts
    
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
```

### Tasks

- [x] Define API-level feature registry under `docs/spec/features/`
- [x] Refactor backend: split `spaces.py` into feature-specific files
- [x] Refactor frontend: organize stores by feature
- [x] Rename `schemas.py` → `form.py` (part of Checkpoint 1)
- [x] Create path verification test for each module
- [x] Add CI check for feature path consistency

### Acceptance Criteria

- [x] `docs/spec/features/` defines API operations with paths + symbol locations
- [x] All modules follow the defined path structure
- [x] Automated tests verify path consistency

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
├── entry.yaml              # REQ-ENTRY-* requirements
├── index.yaml             # REQ-IDX-* requirements
├── integrity.yaml         # REQ-INT-* requirements
├── security.yaml          # REQ-SEC-* requirements
├── api.yaml               # REQ-API-* requirements
├── frontend.yaml          # REQ-FE-* requirements
├── form.yaml              # REQ-FORM-* requirements
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
        - file: ieapp-cli/tests/test_space.py
          tests:
            - test_create_space_scaffolding
            - test_create_workspace_s3_unimplemented
        - file: ieapp-cli/tests/test_indexer.py
          tests:
            - test_indexer_run_once
            
  - id: REQ-STO-002
    title: Space Directory Structure
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
- [x] Implement `test_requirements.py` (requirement coverage verification)
- [x] Implement `test_features.py` (feature path verification)
- [x] Add test name convention: `test_<feature>_<requirement_id>_<description>`
- [x] Update CI to run document verification tests

### Acceptance Criteria

- [x] All requirements converted to YAML format
- [x] Document tests verify 100% requirement coverage
- [x] Orphan tests (no requirement) are identified and reviewed
- [x] CI fails if requirements are not covered

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
│   └── directory-structure.md    # Space layout
├── api/
│   ├── rest.md                   # REST API documentation
│   ├── mcp.md                    # MCP protocol documentation
│   └── openapi.yaml              # OpenAPI specification
├── requirements/
│   ├── README.md                 # Requirements format
│   ├── storage.yaml
│   ├── entry.yaml
│   └── ... (per category)
├── security/
│   ├── overview.md               # Security strategy
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
    i_want: to query my entries through MCP resources
    so_that: I can perform complex tasks like analyzing data and generating reports
    acceptance_criteria:
      - MCP resources expose entry lists and content
      - AI can query structured properties via API
      - Output (text/charts) is returned to AI context
    related_apis:
      - MCP resource: ieapp://{space_id}/entries/list
      - REST: POST /spaces/{space_id}/query
    requirements:
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

- [x] All content migrated to new structure
- [x] YAML files are valid and parseable
- [x] Cross-references are updated and verified
- [x] `docs/tests/README.md` documents the test framework

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

- [x] All checkpoints completed with acceptance criteria met
- [x] All tests pass (unit, integration, e2e)
- [x] Documentation updated and consistent
- [x] CI/CD includes document verification tests
- [x] No regressions in existing functionality
- [x] README.md updated to reflect new structure

---

## References

- [Roadmap](../roadmap.md) - Future milestones
- [MVP Archive](mvp-milestone.md) - Previous milestone
- [Architecture Overview](../../spec/architecture/overview.md) - System design (TO-BE)
