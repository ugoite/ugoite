# IEapp Development Roadmap

**Vision**: "Local-First, AI-Native Knowledge Space for the Post-SaaS Era"  
**Core Principles**: Low Cost, Easy, Freedom

This roadmap outlines the major milestones planned for IEapp development.

---

## Milestone Overview

| # | Milestone | Status | Description |
|---|-----------|--------|-------------|
| 1 | **MVP** | ✅ Completed | Minimum Viable Product - Core functionality |
| 2 | **Full Configuration** | ✅ Completed | Codebase unification and architecture refinement |
| 3 | **Markdown as Table** | 📋 Planned | Store notes as Class-backed Iceberg tables with SQL querying |
| 4 | **User Controlled View** | 📋 Planned | User-defined UI views driven by queries |
| 5 | **AI-Enabled & AI-Used** | 📋 Planned | Complete MCP integration and AI workflow features |
| 6 | **User Management** | 📋 Planned | Authentication, authorization, and multi-user support |
| 7 | **Native App** | 📋 Planned | Desktop application with Tauri |

---

## Milestone 1: MVP ✅

**Archive**: [archive/mvp-milestone.md](archive/mvp-milestone.md)

Delivered the foundational architecture and core features:
- Local-first storage with fsspec abstraction
- Note CRUD with revision history and conflict detection
- Markdown-to-structured-data extraction
- Schema/Class definitions for typed notes
- SolidStart frontend + FastAPI backend

---

## Milestone 2: Full Configuration ✅

**Archive**: [archive/milestone-2-full-configuration.md](archive/milestone-2-full-configuration.md)

Focus on codebase quality, consistency, and architecture refinement:

### Key Objectives
1. **Terminology Unification** - Consolidate "datamodel", "schema", "class" to single "class" term
2. **Rust Core Library** - Extract core logic into a Rust crate for multi-platform use
3. **Feature Path Consistency** - Standardize directory structure across all modules
4. **Requirements Automation** - YAML-based requirements with automated test verification

### Deliverables
- `ieapp-core` Rust crate with opendal for storage
- Python bindings via pyo3 for ieapp-cli
- Unified feature paths in `docs/spec/features.yaml`
- YAML-based requirements in `docs/spec/requirements/`
- Document consistency tests

---

## Milestone 3: Markdown as Table 📋

**Tasks**: [tasks.md](tasks.md)

Focus on storing notes as Class-backed Iceberg tables while preserving the current UI behavior:

### Key Objectives
1. **Iceberg Storage** - Class-defined fields stored as Iceberg tables per Class
2. **Class-First Notes** - Notes require a Class; no classless notes
3. **Deterministic Reconstruction** - Markdown can be reconstructed from table rows
4. **IEapp SQL** - Domain-specific SQL for flexible user queries

### Expected Deliverables
- Iceberg-backed storage in `ieapp-core`
- Class validation for allowed fields
- SQL query engine over Class data

---

## Milestone 4: User Controlled View 📋

Focus on enabling user-defined UI views driven by queries:

### Key Objectives
1. **Query + UI Composition** - Users attach UI components to queries
2. **Low-Code Views** - Views are expressed as UI-only definitions
3. **Shareable View Specs** - Views stored in the workspace and reusable

### Expected Deliverables
- View definition format and renderer
- Query-driven UI panels
- Saved, shareable view definitions

---

## Milestone 5: AI-Enabled & AI-Used 📋

Focus on complete AI integration and workflow automation:

### Key Objectives
1. **Full MCP Server Implementation** - Complete resource and tool exposure
2. **AI Workflow Automation** - Scheduled tasks, batch processing via AI
3. **Vector Search Integration** - FAISS index with embedding support
4. **Voice-to-Schema** - Audio upload with AI-powered transcription and structuring
5. **Computational Notebooks** - Embedded JavaScript execution in notes

### Expected Deliverables
- Production-ready MCP server
- Vector search with configurable embedding providers
- Voice memo attachment with transcription workflow
- Interactive code block execution in editor

---

## Milestone 6: User Management 📋

Focus on multi-user support and security:

### Key Objectives
1. **Authentication** - JWT/OAuth2 support with configurable providers
2. **Authorization** - Workspace and note-level permissions
3. **Multi-tenant Workspaces** - Shared workspaces with collaboration
4. **Audit Logging** - Track all changes with user attribution
5. **API Keys** - Service account access for automation

### Expected Deliverables
- Pluggable auth provider system
- Role-based access control (RBAC)
- Workspace sharing and invitation system
- Activity audit trail

---

## Milestone 7: Native App 📋

Focus on desktop application using Tauri:

### Key Objectives
1. **Tauri Desktop App** - Cross-platform (Windows, macOS, Linux)
2. **Direct Crate Integration** - ieapp-core used directly (not via Python)
3. **Offline-First Sync** - Background sync with conflict resolution
4. **System Integration** - File associations, tray icon, keyboard shortcuts
5. **Mobile Support** - iOS/Android via Tauri Mobile (experimental)

### Expected Deliverables
- Standalone desktop application
- Local-only mode with no server dependency
- Optional cloud sync via storage connectors
- Mobile companion apps (experimental)

---

## Timeline (Tentative)

```
2026 Q2  |████████████████| Milestone 3: Markdown as Table
2026 Q3  |████████████████| Milestone 4: User Controlled View
2026 Q4  |████████████████| Milestone 5: AI-Enabled & AI-Used
2027 Q1  |████████████████| Milestone 6: User Management
2027 Q2  |████████████████| Milestone 7: Native App
```

---

## Contributing

Contributions are welcome! For each milestone:
1. Review the current [tasks.md](tasks.md) for active work items
2. Check [archive/](archive/) for completed milestones and lessons learned
3. Open an issue to discuss larger changes before submitting a PR

---

## Related Documentation

- [Specification Index](../spec/index.md) - Technical specifications
- [Architecture Overview](../spec/architecture/overview.md) - System design
- [AGENTS.md](../../AGENTS.md) - AI Agent development guide
