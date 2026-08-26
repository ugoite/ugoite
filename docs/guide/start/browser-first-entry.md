---
title: "Create the first browser entry"
sidebar:
  order: 4
---

> Supported v0.1 workflow. The browser is server-backed and uses passwordless
> Passkey/WebAuthn authentication with an opaque session cookie.

Complete the one-use setup URL printed by the server, register the initial
Passkey, and use the login page for subsequent passwordless sessions. See the
[authentication operator guide](../operate/auth/auth-overview.md) for the
bootstrap and session contract. Browser-local persistence and optional sync
remain future work.

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
