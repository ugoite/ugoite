---
title: "Product Principles"
description: The six product decisions that shape every Ugoite surface.
sidebar:
  order: 2
---

Product Principles name what Ugoite will not trade away while it grows. They
guide Browser, CLI, REST, MCP, and Konase together, so the same Knowledge
operation keeps the same meaning everywhere.

## Own your Knowledge

Durable content lives in an operator-owned Space and remains portable across the
runtimes that work with it. A deployment or storage operator may host a Space
without becoming its authority.

## One Knowledge model, many surfaces

Browser, CLI, REST, MCP, and Konase operate on the same Space-owned Knowledge
through shared semantics. Surfaces differ in presentation and workflow, not in
what counts as durable.

## Durable things are explicit

What persists is named and inspectable: Spaces, Forms, Entries, Assets, saved
SQL, Changes, and portable history. Derived indexes, query sessions, and runtime
state can be rebuilt and never stand in for authority.

## Recovery is product behavior

History is append-only and recovery never depends on a hidden database. An
operator preserves the Space prefix, the Node control-store prefix, and the node
secret separately, and can reconstruct the Space from those inputs.

## No hidden authority

No server catalog, browser session, model provider, agent runtime, or generated
experience silently owns Knowledge, authorization, or recovery. When Work or
Experience produces a result worth keeping, it is promoted through the normal
Space mutation path.

## Current and future are separated

Current capability, next direction, North Star, and not-promised work are
documented apart. See [Current Product and Target State](current-and-target.md).
A future View or Application Definition may become durable Space content, but
its render cache and transient state do not.

These principles shape the development workflow in
[Develop Ugoite](../develop/index.md) and the authority model in
[Architecture North Star](../architecture/principles/north-star.md).
