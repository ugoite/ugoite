---
title: "Core Concepts"
description: The durable Knowledge model behind Ugoite.
sidebar:
  order: 2
---

Ugoite keeps durable Knowledge in an operator-owned **Space**. Browser, CLI,
REST, MCP, and other experiences use the same Knowledge model; none of them is
an additional source of truth.

## Space

A Space is the ownership, portability, and recovery boundary for Knowledge. It
contains the Forms, Entries, Assets, saved SQL, Changes, and portable history
that a user or operator must be able to move, inspect, and recover together.

The Space is authoritative. A server may host it and a storage operator may
manage its location, but neither becomes the owner of the Knowledge. See the
[Space and storage overview](../architecture/data-model/overview.md) for the
current storage contract.

## Form

A Form describes the fields and validation rules used by Entries. It gives
structured values a stable meaning for validation, filtering, and search while
allowing longer content such as Markdown where that is the better fit.

Forms are durable Space content. A Form change is subject to the current
domain rules; it does not turn a browser editor or a query result into a second
authority.

## Entry

An Entry is a piece of Space-owned Knowledge interpreted through a Form. It has
identity and a revision history. Creating or editing an Entry publishes a new
revision, so the current view can be reconstructed without erasing what came
before it.

## Asset

An Asset is Space-owned file content. Its bytes and integrity metadata are
durable even though the low-level asset lifecycle is independent of Form
definitions. An Entry may own a reference to those bytes through a Form field;
the uploaded reference remains provisional until the normal Entry commit
succeeds. Upload progress, previews, and open file views are temporary Work.
Ugoite does not create a hidden attachment database or universal Entry
attachment property.

## Revision

A Revision is an immutable point in an Entry's history. The current Entry view
is derived from the latest valid revision and its lineage. Edits, deletions,
and restores extend the history. A stale parent revision produces a conflict
instead of silently overwriting a newer change.

## Change, Run, and Undo

A **Change** records a committed Knowledge mutation in the Space timeline. Its
identity and relationship to the append-only publication history make the
mutation portable and attributable.

A **Run** correlates Changes produced by one operation or piece of Work. The
Run is a way to address related changes; it is not a hidden mutable database
record that replaces the Space history.

**Undo** and selective Change revert append inverse Changes. They do not delete
or rewrite the original revisions. Repeating a recovery operation can therefore
continue from the history that is already durable.

## Derived state

Search indexes, query sessions, render caches, open tabs, and execution
progress are derived or runtime state. They may be rebuilt or discarded. A
missing derived relation can reduce search capabilities, but it must not make
the authoritative Space, Entry history, or Asset bytes disappear.

## Durable Knowledge

The durable boundary is the set of Space-owned facts that remain meaningful
after a browser session, CLI process, server instance, model interaction, or
generated experience goes away:

- Spaces and Forms;
- Entries, Assets, and their revisions;
- saved SQL and committed Changes; and
- portable history needed to reconstruct the Knowledge state.

This is the product boundary in one sentence: **Knowledge persists. Work may
disappear. Knowledge can become tools.** Work and Experience can help create or
use Knowledge, but a result becomes durable only through the normal Space
mutation path.

## Current boundaries

The browser is currently server-backed. Browser-local persistence and optional
synchronization are planned, not shipped. View and Application Definitions,
renderers, a general application builder, arbitrary code execution, and hidden
app-specific authorization are also future or not-promised capabilities.

For the distinction between durable content, temporary Work, and future
Experience, read [Knowledge, Work, and Experience](knowledge-work-experience.md)
and [Current Product and Target State](current-and-target.md).
