---
title: "Knowledge, Work, and Experience"
description: What must survive, what may disappear, and what makes Knowledge useful.
sidebar:
  order: 3
---

Ugoite separates what must survive from what is useful only while someone, or an
agent, is doing a task. The boundary is a product principle, not a requirement
for a particular UI or AI provider.

## Knowledge persists

Knowledge is the durable, inspectable content a user owns in a Space: Forms,
Entries, Assets, saved SQL, Changes, and portable history. The
operator-controlled Space is the authority. A server may authenticate or serve
it, a model may reason over it, and an adapter may render it, but none of those
layers becomes the owner.

## Work may disappear

Work is the temporary state of trying to understand or change Knowledge: model
interaction, temporary context, observations, intermediate reasoning, execution
progress, retries, and tool results. Konase is one Work runtime; its state and
agent memory may be discarded without changing Knowledge authority.

When a result matters, a user or authorized host promotes it to Knowledge
through the normal mutation path, with the same Change and Undo behavior as any
other durable mutation.

## Experience makes Knowledge useful

Experience is the layer that makes Knowledge useful for a purpose: a table,
dashboard, research view, data-entry screen, search interface, or project
workspace. Experience runtime state such as open tabs, render caches, and
transient results is replaceable.

A future View or Application Definition may become durable Space content, but it
does not require arbitrary code execution, package installation, an app-specific
backend, hidden durable state, or a second authorization authority.

For the conceptual boundary and its failure model, see
[Knowledge, Work, and Experience](../architecture/principles/knowledge-work-experience.md).
For the product direction, see
[Current Product and Target State](current-and-target.md).
