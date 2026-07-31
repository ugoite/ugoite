---
title: "Develop Ugoite"
description: Run the repository from source and complete the local authentication path.
sidebar:
  label: "Overview"
  order: 1
---

Development starts at the repository root and uses the same Rust server,
frontend, docsite, and validation tasks as CI. The browser remains server-backed
in the current release.

## Development path

1. Follow [Local development login](local-dev-auth-login.md) to start from
   source and complete the first-run authentication flow.
2. Use the repository root `mise` tasks for formatting, checks, tests, and the
   docsite build.
3. When changing the browser or API boundary, read the matching
   [architecture](../../architecture/index.md) and
   [executable specification](../../spec/index.md).
