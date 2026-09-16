---
title: "Get started"
description: The shortest path from an empty machine to a working Ugoite Space.
sidebar:
  label: "Overview"
  order: 1
---

Start here if you are new to Ugoite. You will understand the operator-owned
Space model, launch the smallest useful deployment, and complete the first
durable Knowledge workflow. The browser is currently server-backed; the Space,
rather than the browser session, remains the Knowledge authority.

## Follow this order

1. Read [Vision & Core Concepts](../vision/core-concepts.md) to understand the
   Space authority boundary and the Knowledge, Work, and Experience model.
2. Complete the [Quickstart](quickstart.mdx) once: one golden-journey page
   covers Space, Form, Entry, Edit, Search, History, and Restore with Browser
   and CLI on the same page.
3. Continue with [Knowledge tasks](../use/index.md) for everyday Knowledge tasks.
4. Choose [Operations](../operate/install-deploy.md) when you need to
   install or run a deployment.

Browser and CLI reach the same Knowledge outcome with equivalent meaning. CLI
core and remote differences are noted inline only where they change what to
type. There is no separate Browser, CLI, or REST tree: each task page keeps
all surfaces together.

## Authorities on this path

- CLI syntax: the current binary via `ugoite <command> --help`.
- REST shapes: `crates/ugoite-server` and `/openapi.json`.
- Requirements and evidence: Mitase declarations. Docs explain; they do not
  create a second protocol registry.

## After the first run

- Continue with [Development](../develop/index.md) to run Ugoite from source.
- Keep [Current Product and Target State](../vision/current-and-target.md)
  nearby to separate Current from Planned and North Star.
