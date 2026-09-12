---
title: "Create the first browser entry"
sidebar:
  order: 4
---

> Supported v0.1.1 workflow. The browser is server-backed and uses passwordless
> Passkey/WebAuthn authentication with an opaque session cookie.

## Start in the browser

Open the server URL in a browser. On the first start, use the one-use setup URL
printed in the server or container log. Register the initial Passkey, save the
bootstrap recovery codes, and register a second Passkey before leaving setup.
On later visits, sign in with a registered Passkey. See the
[authentication operator guide](../operate/auth/auth-overview.md) for the
bootstrap, recovery, and session contract.

## Create and edit an Entry

1. Select a Space, open **Entries**, and choose **New Entry**.
2. Choose the Form for the Entry. In the **Fields** tab, fill typed fields
   such as short text, number, or date values. Use a Markdown field for longer
   notes, lists, links, and formatted prose.
3. Use **Preview** to check rendered Markdown, then choose **Save entry**.
   The saved/unsaved status and any validation error are shown on the page.
4. To inspect the complete Markdown representation, open the advanced
   **Source** tab. This is still available for the legacy Markdown authoring
   path; existing raw Markdown Entries remain supported in v0.1.x.

The Fields view is the normal structured-first path. Form-defined fields and
the Markdown source represent the same Entry; switching views does not create
a second copy of the Knowledge.

## Search, history, and restore

Use **Search** to find an Entry by keyword or structured criteria. Open the
result to return to its Entry detail page. From there, open **History** to
review revisions and choose **Restore** for a prior revision. Restore creates a
new append-only revision containing the selected content; it does not erase the
existing history.

Browser-local persistence and optional sync remain future work. The browser is
server-backed, while the Space remains the durable Knowledge authority.

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
