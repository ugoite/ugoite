# Core Concepts: Spaces, Entries, Forms, and Search

Use this guide when you want the plain-language mental model before choosing
the browser, CLI, or deeper specification docs.

Ugoite is built to keep your knowledge local-first, portable, and easy to
understand. The core idea is simple: you keep information in a **space**,
organize it as **entries**, define predictable structure with **forms**, and
then let Ugoite derive **search and indexes** from that source data.

In one sentence: a space groups entries, forms define their structure, Markdown
stays the authoring surface, and search is derived from the typed fields Ugoite
extracts when you want schema-aware behavior.

## Start with a space

A **space** is the top-level container for your knowledge. Think of it as a
portable workspace that owns its own entries, forms, settings, assets, and
derived indexes.

In practice, a space is still just data you control. Ugoite stores it in a
directory structure that can live on local disk today and move to other storage
backends later. That is the "local-first" part of the philosophy: your data is
not trapped behind a hosted database or a proprietary SaaS account.

Today, that local-first promise is strongest in the storage layer and in CLI
`core` mode, where commands talk to the local filesystem directly. The current
browser UI still depends on a running backend/proxy and explicit login; it uses
local-first storage underneath, but it is not yet a backend-free browser mode.

If you are evaluating Ugoite, a good first question is: "What should be its own
space?" A team wiki, a research notebook, or a project knowledge base are all
reasonable examples.

## Entries are the actual content

An **entry** is one piece of content inside a space. It might represent a note,
a task, a meeting log, a person, or any other record you want to keep.

Entries are friendly to Markdown, but Ugoite also stores them in table-backed
storage so they stay queryable and structured. You can think of an entry as:

- a human-readable document when you edit it
- a row in a table when you search or automate it
- a versioned record when you need history and auditability

This dual view is important. Ugoite does not force you to choose between "plain
text notes" and "structured database rows." It tries to give you both.

## Forms define the structure

A **form** defines the expected shape of an entry type. If a space contains
"Meeting" entries and "Task" entries, each of those can have its own form.

Forms tell Ugoite things like:

- which fields exist
- which fields are required or optional
- what type each field has
- what template new entries should start from

That makes forms the bridge between free-form writing and reliable structure.
They help people enter consistent information, and they help the browser, CLI,
and automation flows understand what each entry is supposed to contain.

For browser-first users, that makes the first-run workflow concrete: create or
open a space, create a form, then create entries that use it. A new space
becomes meaningfully authorable once at least one form exists, because the form
is what tells Ugoite how new structured entries should start.

You can absolutely start with a lightweight note, but as soon as you want
stable extracted fields, validation, or reliable queries, the Form becomes the
contract Ugoite uses to interpret that Markdown.

## Markdown becomes structured data

One of Ugoite's key ideas is **Markdown as Table**.

When you write an entry, Markdown headings and frontmatter can map onto the
fields defined by a form. That means you keep a writing-friendly editing model
without giving up typed fields, validation, or queryability.

For a newcomer, the important takeaway is this: Markdown remains the authoring
surface, but the structure is still governed by the active Form definition when
you want typed fields. You are not filling out a rigid web form first and
exporting later, but you also are not bypassing schema contracts entirely once
you ask Ugoite to extract predictable structure.

## Search and indexes are derived, not primary

Search, filtering, and other indexes are derived from the entries and forms in a
space. They help the UI and CLI answer questions quickly, but they are not the
source of truth.

That design matters because it keeps the system easier to reason about:

- entries and forms are the canonical domain data
- indexes can be rebuilt when needed
- automation can trust that search results come from local-first source data

In other words: Markdown stays the authoring surface, entries plus Forms define
the logical contract, and storage/index internals exist to persist or accelerate
that contract rather than replace it.

## Which surface should you use?

Once the concepts make sense, choose the surface that matches your task:

- Use the [Container Quick Start](container-quickstart.md) when you want the
  fastest browser-based evaluation and you are comfortable running the backend
  + frontend stack locally.
- Use the [CLI Guide](cli.md) when you prefer terminal-first workflows or
  scripting, or when you want the thinnest local-first workflow today.
- Use the [REST API](../spec/api/rest.md) when you are integrating another
  client or intentionally want server-backed automation instead of direct local
  filesystem access.
- Use the [Docker Compose Guide](docker-compose.md) when you want the full
  contributor stack from source.
- Use the [Specification Index](../spec/index.md) when you need the formal
  contracts behind the product.

For the deeper storage and schema details behind these concepts, continue to the
[Data Model Overview](../spec/data-model/overview.md).
