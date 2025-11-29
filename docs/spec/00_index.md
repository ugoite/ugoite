# IEapp Specification: Master Index

**Version**: 2.0.0 (Draft)
**Date**: 2025-11-29
**Status**: Active Development
**Concept**: "Your Data, Anywhere, AI-Programmable. Structured Freedom."

## Vision
**"Low Cost, Easy, Freedom"**

IEapp is a **Local-First, AI-Native Knowledge Space** designed for the post-SaaS era. It rejects the "walled garden" database model in favor of **absolute data ownership**, **portability**, and **direct programmability**.

It solves the "Markdown vs. Database" paradox by treating **Markdown Sections as Fields**. You write standard Markdown headers (`## Date`, `## Attendees`), and the system automatically extracts them as structured data. You can define "Classes" (Schemas) to enforce structure and templates, giving you the power of a database with the simplicity of a text file.

Unlike traditional apps that offer limited "tools" to AI, IEapp provides a **Code Execution Environment** via the Model Context Protocol (MCP). This allows your AI assistant to not just read your notes, but to *program* your knowledge base—analyzing data, generating charts, or refactoring content using Python.

## Specification Documents

This specification is broken down into the following documents:

| Document | Description |
|----------|-------------|
| [01_architecture.md](./01_architecture.md) | System architecture, tech stack, and the "Code Execution" paradigm. |
| [02_features_and_stories.md](./02_features_and_stories.md) | User scenarios, including "Trendy" features and AI interactions. |
| [03_data_model.md](./03_data_model.md) | The `fsspec` file layout, JSON schemas, and versioning strategy. |
| [04_api_and_mcp.md](./04_api_and_mcp.md) | REST API endpoints and the MCP (Model Context Protocol) definition. |
| [05_security_and_quality.md](./05_security_and_quality.md) | Security measures, testing strategy (TDD), and error handling. |

## Quick Summary of Key Decisions

*   **Storage**: `fsspec` (Local, S3, etc.) with a JSON-based schema. No RDB.
*   **AI Interface**: MCP Server with **Python Code Execution** capability.
*   **Frontend**: SolidJS + Bun (Local-first, Optimistic UI).
*   **Backend**: Python FastAPI (Stateless, acting as the MCP host).
*   **Search**: Local FAISS vector index + Inverted Index.
*   **Testing**: Pytest (Backend) + Playwright (E2E).

