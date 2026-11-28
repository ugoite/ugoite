---
title: "Current Product and Target State"
description: What is current, what is next, what is North Star, and what is not promised.
sidebar:
  order: 4
---

This page keeps four time horizons apart. Current documentation describes the
implementation represented by this documentation build and may include
unreleased changes. Planned pages carry a Planned badge. North Star describes
direction. Not-promised work is named so it cannot be mistaken for a roadmap
commitment. The [release-scope page](../architecture/release/release-scope.md)
defines the Current and Stable documentation boundary.

## Current

This documentation build describes the current implementation on the v0.1
authority boundary for operator-owned Spaces:

This is a product-level overview, not an exhaustive feature inventory. Use the
[current release scope](../architecture/release/release-scope.md) for the full
Current-versus-Stable boundary and the versioned release notes for published
capabilities.

The implementation includes local Rust CLI operations, a Rust server with a
server-backed browser, the small authenticated MCP facade, and Text-first
Composition authoring and runtime. Composition remains unreleased; the stable
v0.2.1 boundary and the current main implementation are distinguished in the
[release scope](../architecture/release/release-scope.md). The user-owned Space
remains the Knowledge authority and revisions remain append-only.

## Next

The cross-surface acceptance gate follow-up remains tracked in
[#2563](https://github.com/ugoite/ugoite/issues/2563).

The v0.3 direction is **Authority → Experience → Work**: map what existing
Space-owned Knowledge can do and where each behavior is specified, implemented,
and verified before designing owned Experiences. It does not move Application
Definitions, renderers, or execution behavior into Current.

## North Star

Humans and agents compose portable, inspectable Views and task-specific tools
from the same Space-owned Knowledge. Durable definitions, when introduced,
remain Space content; rendered and execution state remains runtime state.

The target does not require arbitrary code execution, arbitrary package
installation, an app-specific backend or database, hidden durable application
state, or an application-specific authorization authority.

## Not promised

Browser-local persistence and optional synchronization, View and Application
definitions, general Knowledge-to-tools renderers, a general-purpose low-code
application builder, and an arbitrary code runtime are not shipped. Here,
low-code means general-purpose application building; it does not mean the
Text-first Composition editor/runtime described in the
[release scope](../architecture/release/release-scope.md).
No general application builder or arbitrary code runtime is claimed as current.

See [The Ugoite Vision](index.md) for the promise and
[Design History](design-history.md) for why Vision stays separate from
implementation detail.
