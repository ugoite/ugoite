---
title: "Knowledge, Work, and Experience"
sidebar:
  order: 3
---

Ugoite separates what must survive from what is useful only while someone—or an
agent—is doing a task. The boundary is a product principle, not a requirement
for a particular UI or AI provider.

## Knowledge persists

Knowledge is the durable, inspectable content that a user owns in a Space:
Forms, Entries, Assets, saved SQL, Changes, portable history, and any future
durable View or Application Definition. The operator-controlled Space is the
authority for this content. A server may authenticate or serve it, a model may
reason over it, and an adapter may render it, but none of those layers becomes
the owner.

## Work may disappear

Work is the temporary state of trying to understand or change Knowledge. It can
include model interaction, temporary context, observations, intermediate
reasoning, execution progress, retries, and tool results. Konase is one Work
runtime; its state, agent memory, and provider-specific context are not a
durable transcript or a second Knowledge store.

Work may be discarded after completion or failure without making the Space
unrecoverable. When a result matters, a user or authorized host promotes it to
Knowledge through the existing mutation path. That promotion produces the same
Change, Run correlation, and Undo behavior as any other durable mutation.

## Knowledge can become tools

Experience is the layer that makes Knowledge useful for a purpose: a table,
dashboard, research view, structured data-entry screen, search interface,
project workspace, or domain-specific mini application. Humans and agents may
help compose these experiences, but Experience is not a new system of record.

The target is portable, inspectable, task-specific tools. It is not a general
application-hosting runtime inside Ugoite. In particular, this principle does
not require arbitrary JavaScript or Python execution, package installation, an
app-specific backend or database, hidden durable application state, or a second
authorization authority.

## Durable definitions and runtime state

A future View or Application Definition may be saved as Space-owned Knowledge.
It can describe the Forms and queries it uses, component composition, filters,
navigation, permitted actions, and presentation metadata. The currently open
tab, scroll position, render cache, transient query result, and component
instance state remain runtime state and do not become Knowledge authority.

## Failure and recovery

If a Konase host, model provider, browser renderer, or generated Experience
fails, the operator must still be able to recover the Space and its durable
history from operator-controlled storage. Experience writes cannot bypass
authorization or append-only publication rules. This is why generated tools do
not copy Knowledge into an opaque application database and why a successful
render is never evidence of durable persistence.

The earlier Canvas and code-sandbox experiments are design history, not a
request to restore those runtimes. They helped expose the durable requirement:
Knowledge should remain user-owned while different tools can be built around
it.
