---
title: "System boundaries"
description: Where shared Rust behavior ends and CLI, server, browser, and WASM adapters begin.
sidebar:
  label: "Boundaries"
  order: 2
---

Ugoite has one application model and several adapters. This group explains the
interfaces that keep local use portable while allowing the browser and server to
evolve independently.

- [Runtime adapters](runtime-adapters.md) covers core mode, API mode, browser,
  and WASM responsibilities.
- [Portable API client](portable-api-client.md) defines the transport-neutral
  operation protocol.
- [Frontend client boundary](frontend-client-boundary.md) explains what the
  browser owns and what it delegates to the server/WASM boundary.
