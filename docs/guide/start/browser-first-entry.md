---
title: "Create the first browser entry"
sidebar:
  order: 4
---

> Future/reference workflow. The browser is server-backed, but interactive
> Passkey/TOTP/OIDC authentication is not a supported v0.1 capability.

This page intentionally does not provide browser setup or login instructions.
For the supported v0.1 workflow, operate the authoritative Space directory with
the local CLI. Browser-local persistence and optional sync remain future work.

## Form field choices

Use short text/number/date fields for values that should sort, filter, or
validate predictably. Use a Markdown field for longer notes, lists, links, and
formatted prose. Both remain part of the Form-defined Entry; the distinction is
about editing and query behavior.

## Main browser surfaces

- **Dashboard** summarizes the selected Space.
- **Entries** lists and edits content and revision history.
- **Forms** defines typed fields used by Entries.
- **Search** performs keyword and structured query workflows.
- **SQL** stores reusable SQL definitions and opens query results.
- **Settings** shows Space storage and membership controls allowed by the
  current role.
