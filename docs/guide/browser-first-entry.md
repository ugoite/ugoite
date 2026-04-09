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

## 2) Create the first form

When the space dashboard opens, the browser UI guides you toward the first form.
If entry creation is still disabled, that is expected: the current browser flow
uses a Form to define the first structured entry shape.

Click either:

- **Create first form** from the entry guidance warning, or
- **Create form** / **New Form** from the dashboard or Forms view.

In the form dialog:

1. Enter a **Form name** such as `notes`.
2. Click **Add Column** at least once.
3. Add one or two starter fields such as:
   - `summary` with type `string`
   - `next_steps` with type `markdown`
4. Click **Create**.

Why this matters: the Form gives the browser a predictable entry shape, keeps
fields queryable later, and makes the first entry easier to extend without
guessing the structure every time.

## 3) Create the first entry

Now create content from the structure you just defined:

1. Click **New Entry** on the dashboard or the Entries view.
2. Choose the Form you just created.
3. Enter a title such as `Kickoff Notes`.
4. Fill any required fields.
5. Submit the dialog to create the entry.

The browser should open the new entry detail page. From there you can keep
editing Markdown content, save revisions, and confirm that the form-backed
fields are now part of the entry's structured shape.

## 4) Confirm the browser workflow is now unlocked

After that first successful entry, you should be able to move through the main
space surfaces without guesswork:

- **Dashboard**: quick-create another entry or form.
- **Entries**: browse the records in the space.
- **Forms**: refine the schema and add more entry types.
- **Search**: use the top tab to verify that the space is now discoverable
  beyond a single page.

## 5) Where to go next

- Need the mental model behind spaces, entries, forms, and derived structure?
  Read [Core Concepts](concepts.md).
- Need to inspect storage or other space configuration? Use the **Settings**
  link from the spaces list or the dashboard storage summary card.
- Need browser/CLI/API auth context after the first entry? Read
  [Authentication Overview](auth-overview.md).
- Want the lighter local-first path after trying the browser once? Switch to the
  [CLI Guide](cli.md).
