---
title: "Architecture & Specification"
description: Architecture, vision, requirements, policies, and evidence for Ugoite.
sidebar:
  order: 1
---

**Updated:** 2026-09-12\
**Implementation status:** Rust/Deno v0.1.1 published

Ugoite is a private, portable Knowledge Space for humans and AI. Its foundation
is expressed as three boundaries: **Knowledge persists**, **Work may
disappear**, and **Knowledge can become tools**. User-owned Space directories
are authoritative; deployment and storage operators may host them without
becoming Knowledge authority. Indexes, query sessions, and runtime state are
derived or disposable.

## Current boundary

- Local CLI core mode directly opens Spaces.
- The Rust server exposes REST, the small MCP v1 semantic facade, and static
  browser hosting.
- The browser is server-backed.
- v0.1 supports mandatory authentication, Passkey/WebAuthn login, opaque
  sessions, owner-approved recovery, recovery-only TOTP Self-Recovery, Remote
  CLI device auth, membership and ACL enforcement, authenticated MCP access,
  authorized audit reads, and invitation-gated OIDC.
- Browser-local persistence and optional sync are planned. Administrator
  recovery, agent principals, generic OAuth compatibility, audit CRUD, and
  remote CLI asset upload remain future or limited. TOTP is recovery-only.
- View and Application Definitions, renderers, and Knowledge-to-tools runtime
  behavior are future scope.

## Specification map

- [Product requirements](requirements/index.md) and
  [user stories](stories/index.md).
- [Features and implementation bindings](features/index.md).
- [Policies](policies/index.md) for governance traceability.
- [Architecture](../architecture/index.md) for boundaries, contracts,
  data model, security, and testing.
- [Vision](../vision/index.md) for the product promise and the
  [Current Product and Target State](../vision/current-and-target.md) that
  keeps Current, Planned, and North Star apart.

The specification is organized by the question it answers. Behavior changes ship
with implementation and verification evidence; incomplete evidence stays an
explicit gap rather than a rewritten requirement.

## Sources of truth

- REST implementation and contract: `crates/ugoite-server` and `/openapi.json`.
- Portable operation contract: `crates/ugoite-api-client`.
- Application behavior: `crates/ugoite-core`.
- Storage behavior: `crates/ugoite-storage` plus core modules.
- Browser behavior: `frontend` (currently server-backed).
- Task and CI surface: root `mise.toml`, `deno.json`, and
  `.github/workflows/ci.yml`.

Machine-readable registries must reference existing source and test paths.
Planned capability stays labeled planned.
