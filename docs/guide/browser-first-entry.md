# Browser Walkthrough: First Space, Form, and Entry

Use this guide after you have already completed the browser login flow and can
reach `/spaces`. It is the concrete post-login path for the browser route's
**time-to-first-entry** experience.

If you have not signed in yet, start with either
[Container Quick Start](container-quickstart.md) or
[Local Development Authentication and Login](local-dev-auth-login.md) first.

## 1) Start from `/spaces`

Open the spaces list after login:

```text
http://localhost:3000/spaces
```

From there, take the path that matches what you see:

- **Published quick start**: the shipped browser stack usually gives you a ready
  `default` space. Click **Open Space** on that card.
- **Source development or an empty browser session**: click **Create space**,
  choose a short ID such as `team-notes`, then submit **Create space**.

If `admin-space` is visible, treat it as the reserved admin workspace. Create or
open a normal user-facing space for your first real content flow.

## 2) Create the first entry with the starter `Entry` form

Fresh spaces already bootstrap a user-creatable `Entry` form, so you do **not**
need to design a custom form before the first note.

1. Click **New Entry** on the dashboard or the Entries view.
2. Choose the starter **Entry** form if the dialog asks for a form.
3. Enter a title such as `Kickoff Notes`.
4. Add the first body content you actually want to keep.
5. Submit the dialog to create the entry.

The browser should open the new entry detail page. That page is the Markdown
editor you will use for follow-up revisions on the same record.

Before moving on, look for the confidence checks that prove the first-run path
worked: the entry detail page is open, the new record can be reopened from
**Entries**, and the space now has content that the derived **Search** surface
can discover.

## 3) Make one follow-up edit in the entry detail editor

1. Stay on the new entry detail page, or reopen the same record from
   **Entries**.
2. Edit the left Markdown pane. The right preview pane updates from the same
   content so you can confirm headings, lists, and emphasis before saving.
3. After you type, look for the **Unsaved changes** indicator in the editor
   toolbar.
4. Click **Save** or press `Cmd/Ctrl+S`.
5. Confirm the **Unsaved changes** indicator disappears and the editor stays on
   the same entry detail page.
6. Open **Entries** again and reopen the record to verify the updated text is still there.

Why this matters: the browser keeps the Markdown editor and preview together, so
the next edit after first-entry creation is the fastest way to confirm that
future revisions are working end to end.

## 4) Add a custom form when you want stricter structure

Once the first note exists, add a custom form if you want queryable fields or a
repeatable template for a specific entry type.

Click either:

- **Create form** / **New Form** from the dashboard or Forms view, or
- the form-specific prompt if the current page surfaces one.

In the form dialog:

1. Enter a **Form name** such as `notes`.
2. Click **Add Column** at least once.
3. Add one or two starter fields such as:
   - `summary` with type `string`
   - `next_steps` with type `markdown`
4. Click **Create**.

Why this matters: a custom Form gives the browser a predictable entry shape,
keeps fields queryable later, and makes follow-up entries easier to extend
without guessing the structure every time.

The example field types are intentionally different: `summary` works well as a
`string` when you want short list-friendly text, while `next_steps` as
`markdown` leaves room for longer formatted follow-up notes.

## 5) Confirm the browser workflow is now unlocked

After that first successful entry, you should be able to move through the main
space surfaces without guesswork:

- **Dashboard**: the landing surface for quick-create actions, recent activity,
  and high-level space status.
- **Entries**: the record list when you want to browse or reopen content that is
  already in the space.
- **Forms**: the Form workspace for adding entry types, refining fields, and
  deciding how future records stay structured.
- **Search**: the derived search surface built from entries and Forms when you
  want to confirm that the space is now discoverable beyond a single page.

## 6) Where to go next

- Need the mental model behind spaces, entries, forms, and derived structure?
  Read [Core Concepts](concepts.md).
- Need to inspect storage or other space configuration? Use the **Settings**
  link from the spaces list or the dashboard storage summary card, then read
  [Space Settings & Storage](space-settings-storage.md) before changing anything.
- Need browser/CLI/API auth context after the first entry? Read
  [Authentication Overview](auth-overview.md).
- Need integrations or server-backed automation next? Read the
  [REST API](../spec/api/rest.md) and then pair it with
  [Authentication Overview](auth-overview.md) for the auth model behind browser,
  CLI, and API clients.
- Want the lighter local-first path after trying the browser once? Switch to the
  [CLI Guide](cli.md).
