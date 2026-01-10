# IEapp Development Roadmap

**Vision**: "Local-First, AI-Native Knowledge Space for the Post-SaaS Era"  
**Core Principles**: Low Cost, Easy, Freedom

This roadmap outlines the major milestones planned for IEapp development.

---

## Milestone Overview

| # | Milestone | Status | Description |
|---|-----------|--------|-------------|
| 1 | **MVP** | ✅ Completed | Minimum Viable Product - Core functionality |
| 2 | **Full Configuration** | 🔄 In Progress | Codebase unification and architecture refinement |
| 3 | **AI-Enabled & AI-Used** | 📋 Planned | Complete MCP integration and AI workflow features |
| 4 | **User Management** | 📋 Planned | Authentication, authorization, and multi-user support |
| 5 | **Native App** | 📋 Planned | Desktop application with Tauri |

---

## Milestone 1: MVP ✅

**Archive**: [archive/mvp-milestone.md](archive/mvp-milestone.md)

Delivered the foundational architecture and core features:
- Local-first storage with fsspec abstraction
- Note CRUD with revision history and conflict detection
- Markdown-to-structured-data extraction
- Schema/Class definitions for typed notes
- Wasm sandbox for AI code execution
- SolidStart frontend + FastAPI backend

---

## Milestone 2: Full Configuration 🔄

**Tasks**: [tasks.md](tasks.md)

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

## Milestone 3: AI-Enabled & AI-Used 📋

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

## Milestone 4: User Management 📋

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

## Milestone 5: Native App 📋

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
2026 Q1  |████████████████| Milestone 2: Full Configuration
2026 Q2  |████████████████| Milestone 3: AI-Enabled & AI-Used
2026 Q3  |████████████████| Milestone 4: User Management
2026 Q4  |████████████████| Milestone 5: Native App
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
