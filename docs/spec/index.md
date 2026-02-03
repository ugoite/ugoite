# IEapp Specification Index

**Version**: 2.0.0 (Full Configuration)  
**Updated**: January 2026  
**Status**: Milestone 2 - In Progress

## Vision

**"Local-First, AI-Native Knowledge Space for the Post-SaaS Era"**

IEapp is a knowledge management system built on three core principles:

| Principle | Description |
|-----------|-------------|
| **Low Cost** | No expensive cloud services required; runs on local storage |
| **Easy** | Markdown-first with automatic structure extraction |
| **Freedom** | Your data, your storage, your AI - no vendor lock-in |

---

## Quick Navigation

### Architecture & Design
- [Architecture Overview](architecture/overview.md) - System design and component responsibilities
- [Technology Stack](architecture/stack.md) - Frontend, backend, and storage technologies
- [Architecture Decisions](architecture/decisions.md) - Key design decisions and rationale
- [Frontend–Backend Interface](architecture/frontend-backend-interface.md) - Behavioral contracts and boundaries
- [Future-Proofing](architecture/future-proofing.md) - Experimental direction (BYOAI, multi-platform core)

### Features & Stories
- [Features Registry](features/README.md) - API-level feature registry across modules
- [IEapp SQL](features/sql.md) - SQL dialect for structured queries
- [Core Stories](stories/core.yaml) - Essential user scenarios
- [Advanced Stories](stories/advanced.yaml) - Power user and experimental features

### UI Specifications
- [UI Overview](ui/README.md) - Page-level UI specifications (YAML)

### Data Model
- [Data Model Overview](data-model/overview.md) - How data is stored and structured
- [Directory Structure](data-model/directory-structure.md) - Workspace layout conventions

### API Reference
- [REST API](api/rest.md) - HTTP endpoints for frontend integration
- [MCP Protocol](api/mcp.md) - AI agent interface via Model Context Protocol
- [OpenAPI Spec](api/openapi.yaml) - Machine-readable API definition

### Requirements
- [Requirements Overview](requirements/README.md) - How requirements are tracked
- Requirements by category: [storage](requirements/storage.yaml) | [note](requirements/note.yaml) | [index](requirements/index.yaml) | [integrity](requirements/integrity.yaml) | [security](requirements/security.yaml) | [api](requirements/api.yaml) | [frontend](requirements/frontend.yaml) | [e2e](requirements/e2e.yaml) | [class](requirements/class.yaml) | [links](requirements/links.yaml) | [search](requirements/search.yaml)

### Security & Quality
- [Security Overview](security/overview.md) - Security strategy and threat model
- [Testing Strategy](testing/strategy.md) - TDD approach and test organization
- [CI/CD](testing/ci-cd.md) - Continuous integration setup
- [Error Handling](quality/error-handling.md) - Error-handling principles and resilience

### Product
- [Success Metrics](product/success-metrics.md) - How we measure progress

---

## Module Responsibility Matrix

| Module | Responsibility | Language |
|--------|----------------|----------|
| `ieapp-core` | Core data operations, storage (OpenDAL), validation | Rust |
| `ieapp-cli` | Command-line interface for direct user interaction | Python |
| `backend` | REST API, MCP server (delegates to ieapp-core) | Python (FastAPI) |
| `frontend` | UI rendering, optimistic updates (no data logic) | TypeScript (SolidStart) |

---

## Key Concepts

### Class
A **Class** defines the structure of a note type. Classes specify:
- Required and optional fields (H2 headers)
- Field types (string, number, date, list, markdown)
- Fixed global template for new notes

### Note
A **Note** is stored as a row in an Iceberg table and can be reconstructed as Markdown:
- H2 sections map to Class-defined fields
- YAML frontmatter carries metadata (class, tags)
- Revision history is stored in the Class `revisions` table

### Workspace
A **Workspace** is a self-contained data directory with:
- Iceberg-managed Class tables and attachments
- Derived indexes regenerated from Iceberg tables
- Portable across storage backends (local, S3, etc.)

---

## Development Resources

- [Tasks](../tasks/tasks.md) - Current milestone work items
- [Roadmap](../tasks/roadmap.md) - Future milestones
- [Contributing](../../AGENTS.md) - Development guidelines

---

## Change History

| Date | Version | Changes |
|------|---------|---------|
| 2026-01 | 2.0.0 | Restructured for Full Configuration milestone; unified terminology to "Class" |
| 2025-12 | 1.0.0 | Initial MVP specification |
