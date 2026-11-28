---
title: "Architecture & Specification"
description: Architecture, vision, requirements, policies, and evidence for Ugoite.
sidebar:
  order: 1
---

Ugoite is a private, portable Knowledge Space for humans and AI. Its foundation
is expressed as three boundaries: **Knowledge persists**, **Work may
disappear**, and **Knowledge can become tools**. User-owned Space directories
are authoritative; deployment and storage operators may host them without
becoming Knowledge authority. Indexes, query sessions, and runtime state are
derived or disposable.

The current implementation and stable release boundaries are documented in
[release scope](../architecture/release/release-scope.md), which points to the
versioned notes for published capabilities.

## Specification map

- Product requirements and verification relationships are canonical in
  [Mitase](https://github.com/ugoite/ugoite/tree/main/docs/mitase). UI page and
  component contracts remain under [UI specifications](ui/index.md). The
  [user stories](stories/index.md) provide product narratives.
- [Features and implementation bindings](features/index.md).
- [Policies](policies/index.md) for governance traceability.
- [Architecture](../architecture/index.md) for boundaries, contracts, data
  model, security, and testing.
- [Vision](../vision/index.md) for the product promise and the
  [Current Product and Target State](../vision/current-and-target.md) that keeps
  Current, Planned, and North Star apart.

The specification is organized by the question it answers. Behavior changes ship
with implementation and verification evidence; incomplete evidence stays an
explicit gap rather than a rewritten requirement.

Earlier registries and migration decisions remain in Git history.

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
