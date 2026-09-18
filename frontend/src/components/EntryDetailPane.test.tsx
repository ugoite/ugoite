// REQ-FE-038: Form validation feedback in editor
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { EntryDetailPane as ActualEntryDetailPane } from "./EntryDetailPane";
import {
  entryApi,
  RevisionConflictError,
  searchApi,
} from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";
import {
  clearCreateEntryDraftSession,
  createEntryDraftSessionKey,
} from "~/lib/create-entry-draft-session";

vi.mock("@solidjs/router", () => ({
  A: (
    props: {
      href: string;
      class?: string;
      title?: string;
      "aria-label"?: string;
      children: unknown;
    },
  ) => (
    <a
      href={props.href}
      class={props.class}
      title={props.title}
      aria-label={props["aria-label"]}
    >
      {props.children}
    </a>
  ),
  useBeforeLeave: () => undefined,
}));

vi.mock("~/lib/ugoite-client", () => {
  class RevisionConflictError extends Error {}
  return {
    entryApi: {
      get: vi.fn(),
      create: vi.fn(),
      update: vi.fn(),
      delete: vi.fn(),
    },
    assetApi: {
      upload: vi.fn(),
      read: vi.fn(),
    },
    searchApi: {
      rowReferenceOptions: vi.fn(),
    },
    RevisionConflictError,
  };
});

// Test fixtures predate the server's stable Form/Field identity response.
// Normalize them at the mocked read boundary so production code still has to
// reject incomplete identity rather than inventing it.
const fixtureUuid = (value: string): string => {
  let hash = 0x811c9dc5;
  for (const char of value) {
    hash = Math.imul(hash ^ char.charCodeAt(0), 0x01000193);
  }
  return `00000000-0000-7000-8000-${
    (hash >>> 0).toString(16).padStart(12, "0")
  }`;
};

const EntryDetailPane = (
  props: Parameters<typeof ActualEntryDetailPane>[0],
) => {
  const sourceForms = props.forms?.() ?? [];
  const ids = new Map<string, string>();
  for (const form of sourceForms) {
    const id = fixtureUuid(`form:${form.name}`);
    ids.set(form.name, id);
    if (form.id) ids.set(form.id, id);
  }
  const normalize = (form: Form): Form => ({
    ...form,
    id: ids.get(form.name) ?? fixtureUuid(`form:${form.name}`),
    fields: Object.fromEntries(
      Object.entries(form.fields ?? {}).map(([name, field], index) => [name, {
        ...field,
        id: field.id && field.id >= 100 ? field.id : 100 + index,
        ...(field.target_form
          ? { target_form: ids.get(field.target_form) ?? field.target_form }
          : {}),
        ...(field.items?.target_form
          ? {
            items: {
              ...field.items,
              target_form: ids.get(field.items.target_form) ??
                field.items.target_form,
            },
          }
          : {}),
      }]),
    ),
  });
  const normalizedForms = sourceForms.map(normalize);
  return (
    <ActualEntryDetailPane
      {...props}
      forms={() => normalizedForms}
      createForm={() => {
        const form = props.createForm?.();
        return form ? normalize(form) : undefined;
      }}
    />
  );
};

describe("EntryDetailPane", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    clearCreateEntryDraftSession(createEntryDraftSessionKey("default"));
    setLocale("en");
  });

  it("REQ-FE-011: synchronizes editor content when the selected entry changes", async () => {
    const [entryId, setEntryId] = createSignal("entry-1");
    (entryApi.get as ReturnType<typeof vi.fn>).mockImplementation(
      async (_spaceId: string, id: string) => ({
        id,
        title: id === "entry-1" ? "First Entry" : "Second Entry",
        form: null,
        content: id === "entry-1" ? "# First Entry" : "# Second Entry",
        revision_id: id === "entry-1" ? "rev-1" : "rev-2",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      }),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={entryId}
        onDeleted={vi.fn()}
      />
    ));

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    expect(textarea).toHaveValue("# First Entry");

    fireEvent.input(textarea, { target: { value: "# Unsaved Draft" } });
    setEntryId("entry-2");

    await waitFor(() => expect(entryApi.get).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(textarea).toHaveValue("# Second Entry"));
  });

  it("REQ-FE-052: edits form fields without requiring Markdown knowledge", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: "Meeting",
      content: "---\nform: Meeting\n---\n\n# Test Entry\n\n## Notes\nhello ",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [
          {
            name: "Meeting",
            version: 1,
            template: "# Meeting\n\n## Date\n\n## Notes\n",
            fields: {
              Date: { type: "string", required: true },
              Notes: { type: "markdown", required: false },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());
    const dateInput = await screen.findByLabelText("Date");
    expect(dateInput).toHaveValue("");
    expect(screen.getByText("This field is required.")).toBeInTheDocument();
    expect(screen.getByLabelText("Notes")).toHaveValue("hello ");

    fireEvent.input(dateInput, { target: { value: " " } });
    expect(dateInput).toHaveValue(" ");
    expect(screen.getByText("This field is required.")).toBeInTheDocument();
    fireEvent.input(dateInput, { target: { value: "2026-07-16" } });
    const notes = screen.getByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "hello \n" } });
    expect(notes).toHaveValue("hello \n");

    const source = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    expect((source as HTMLTextAreaElement).value).toContain(
      "## Date\n2026-07-16",
    );
  });

  it("renders header, a four-action toolbar, then fields with source in disclosure", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-layout",
      title: "Layout Entry",
      form: "Meeting",
      content:
        "---\nform: Meeting\n---\n\n# Layout Entry\n\n## Summary\nhello\n\n## Notes\n**review**",
      revision_id: "rev-layout",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-layout"}
        forms={() => [
          {
            name: "Meeting",
            version: 1,
            template: "# Meeting\n\n## Summary\n\n## Notes\n",
            fields: {
              Summary: { type: "string", required: false },
              Notes: { type: "markdown", required: false },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    await screen.findByLabelText("Summary");

    // Form-bound entries render fields directly with no tab chrome.
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByText("Entry fields preview")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Summary")).toHaveValue("hello");
    // No helper copy under the heading or mode descriptions.
    expect(screen.queryByText(/Edit this entry as a form/)).not
      .toBeInTheDocument();
    expect(screen.queryByText(/familiar form controls/)).not
      .toBeInTheDocument();
    // Sidebar metadata moved to the Info route.
    expect(document.querySelector("#entry-details")).toBeNull();
    expect(document.querySelector(".ui-entry-side-card")).toBeNull();

    // Source editing stays available as an advanced secondary action.
    const advanced = screen.getByText("Advanced");
    expect(advanced.closest("details")).not.toHaveAttribute("open");
    expect(
      await screen.findByPlaceholderText("Start writing in Markdown..."),
    ).toBeInTheDocument();
    const toolbar = screen.getByRole("toolbar", { name: "Entry actions" });
    const tools = toolbar.querySelectorAll(".ui-entry-tool");
    expect(tools).toHaveLength(4);
    // PR4: the single action bar carries save/history/info/delete. Save is
    // weak and disabled while the editor is clean.
    const save = screen.getByRole("button", { name: "Save" });
    expect(toolbar.contains(save)).toBe(true);
    expect(save).toBeDisabled();
    expect(save.classList.contains("ui-entry-tool-primary")).toBe(false);
    expect(screen.getByRole("link", { name: /History & recovery/ }))
      .toHaveAttribute(
        "href",
        "/spaces/default/entries/entry-layout/history",
      );
    expect(screen.getByRole("link", { name: "Info" })).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-layout/info",
    );
    expect(screen.getByRole("button", { name: "Delete entry" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Restore a version/ }))
      .not.toBeInTheDocument();
    // History-only flow: no legacy /restore href remains in the editor.
    expect(
      document.querySelector('a[href$="/entries/entry-layout/restore"]'),
    ).not.toBeInTheDocument();

    // Header → toolbar → fields order.
    const page = document.querySelector(".ui-entry-page")!;
    const header = page.querySelector(".ui-entry-header")!;
    const fields = await screen.findByLabelText("Notes");
    expect(
      header.compareDocumentPosition(toolbar) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      toolbar.compareDocumentPosition(fields) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // PR4: no permanent saved chip in the detail view; success announces
    // once through a transient toast, so no live region is rendered while
    // idle.
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByText("All changes saved")).not.toBeInTheDocument();
    expect(screen.queryByText("Unsaved changes")).not.toBeInTheDocument();
  });

  it("renders formless document entries with the source editor directly", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-doc",
      title: "Doc",
      form: null,
      content: "# Doc",
      revision_id: "rev-doc",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-doc"}
        onDeleted={vi.fn()}
      />
    ));

    const source = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    expect(source).toHaveValue("# Doc");
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(document.querySelector("#entry-source-panel")).not.toBeNull();
  });

  it("uses the shared form-first editor to create a new entry", async () => {
    const onCreated = vi.fn();
    (entryApi.create as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "created-entry",
      revision_id: "created-revision",
    });
    const form: Form = {
      name: "Meeting",
      version: 1,
      template: "# Meeting\n\n## Title\n\n## Summary\n\n## Notes\n\n## Items\n",
      fields: {
        Title: { type: "string", required: false },
        Summary: { type: "string", required: false },
        Notes: { type: "markdown", required: false },
        Items: { type: "list", required: false },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={onCreated}
        onDeleted={vi.fn()}
      />
    ));

    const titleField = await screen.findByLabelText("Title");
    expect(titleField).toHaveValue("");
    fireEvent.input(titleField, { target: { value: "Planning " } });
    expect(titleField).toHaveValue("Planning ");

    const summary = await screen.findByLabelText("Summary");
    fireEvent.input(summary, { target: { value: "Project " } });
    expect(summary).toHaveValue("Project ");

    const notes = await screen.findByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "Details \n" } });
    expect(notes).toHaveValue("Details \n");

    // String lists edit as repeated typed controls: add two items and
    // remove the second before saving.
    fireEvent.click(screen.getByRole("button", { name: "Add item" }));
    fireEvent.click(screen.getByRole("button", { name: "Add item" }));
    const item1 = await screen.findByLabelText("Items item 1");
    fireEvent.input(item1, { target: { value: "one" } });
    fireEvent.input(screen.getByLabelText("Items item 2"), {
      target: { value: "two" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove item 2" }));

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(entryApi.create).toHaveBeenCalled());
    const createPayload = (entryApi.create as ReturnType<typeof vi.fn>).mock
      .calls[0][1] as Record<string, unknown>;
    expect(createPayload.title).toBeUndefined();
    expect(entryApi.create).toHaveBeenCalledWith("default", {
      form: "Meeting",
      tags: [],
      fields: {
        Title: "Planning",
        Summary: "Project",
        Notes: "Details",
        Items: ["one"],
      },
    });
    expect(onCreated).toHaveBeenCalledWith({
      id: "created-entry",
      revision_id: "created-revision",
    });
  });

  it("uses one converged action row for entry creation", async () => {
    const form: Form = {
      name: "Meeting",
      version: 1,
      template: "# Meeting\n\n## Notes\n",
      fields: { Notes: { type: "string", required: false } },
    };
    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreateFormChange={vi.fn()}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));
    await screen.findByLabelText("Notes");

    const page = document.querySelector(".ui-entry-page")!;
    const header = page.querySelector(".ui-entry-header")!;
    // Header carries the single Back navigation and the Form context;
    // no header save area and no badge text.
    expect(header.querySelector('a[href*="/forms"]')).not.toBeNull();
    expect(header.querySelector(".ui-entry-save-area")).toBeNull();
    expect(screen.queryByText("Unsaved changes")).not.toBeInTheDocument();
    expect(screen.queryByText("All changes saved")).not.toBeInTheDocument();
    // Form identity appears once in the selector, not as pill + selector.
    expect(header.querySelector(".ui-pill")).toBeNull();
    expect(screen.getByLabelText("Form")).toBeInTheDocument();

    // Action bar carries the single Save tool. Creating the entry itself
    // is the pending change, so Save starts enabled and strong...
    const bar = page.querySelector(".actionbar.compact-actions")!;
    expect(bar.querySelectorAll(".tool")).toHaveLength(1);
    const save = screen.getByRole("button", { name: "Save" });
    expect(bar?.contains(save)).toBe(true);
    expect(save).toBeEnabled();
    expect(save.classList.contains("ui-entry-tool-primary")).toBe(true);

    // ...and editing keeps it enabled without ever showing badge text.
    fireEvent.input(screen.getByLabelText("Notes"), { target: { value: "hi" } });
    expect(save).toBeEnabled();
    expect(screen.queryByText("Unsaved changes")).not.toBeInTheDocument();
  });

  it("preserves each Form's work when the new-entry Form changes", async () => {
    const formA: Form = {
      name: "Meeting",
      version: 1,
      template: "# Meeting\n\n## Notes\n",
      fields: { Notes: { type: "string", required: false } },
    };
    const formB: Form = {
      name: "Task",
      version: 1,
      template: "# Task\n\n## Status\n",
      fields: { Status: { type: "string", required: false } },
    };
    const [selected, setSelected] = createSignal(formA);

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [formA, formB]}
        createForm={selected}
        onCreateFormChange={(name) =>
          setSelected(name === "Task" ? formB : formA)}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const notes = await screen.findByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "keep this Meeting work" } });
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Task" },
    });
    const status = await screen.findByLabelText("Status");
    fireEvent.input(status, { target: { value: "keep this Task work" } });
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Meeting" },
    });

    await waitFor(() =>
      expect(screen.getByLabelText("Notes")).toHaveValue(
        "keep this Meeting work",
      )
    );
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "Task" },
    });
    await waitFor(() =>
      expect(screen.getByLabelText("Status")).toHaveValue("keep this Task work")
    );
  });

  it("keeps post-request edits in place and binds a completed create to update", async () => {
    let resolveCreate!: (value: { id: string; revision_id: string }) => void;
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockImplementation(
      () => new Promise((resolve) => resolveCreate = resolve),
    );
    const updateMock = entryApi.update as ReturnType<typeof vi.fn>;
    updateMock.mockResolvedValue({ id: "created-entry", revision_id: "rev-2" });
    const onCreated = vi.fn();
    const form: Form = {
      name: "Note",
      version: 1,
      template: "# Note\n\n## Body\n",
      fields: { Body: { type: "string", required: false } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={onCreated}
        onDeleted={vi.fn()}
      />
    ));

    const body = await screen.findByLabelText("Body");
    fireEvent.input(body, { target: { value: "request snapshot" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(1));
    fireEvent.input(body, { target: { value: "typed while saving" } });
    resolveCreate({ id: "created-entry", revision_id: "rev-1" });

    await waitFor(() =>
      expect(screen.getByLabelText("Body")).toHaveValue("typed while saving")
    );
    expect(onCreated).not.toHaveBeenCalled();
    // PR4: the bound draft still reads as dirty work through the save tool.
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(updateMock).toHaveBeenCalledWith(
        "default",
        "created-entry",
        expect.objectContaining({
          parent_revision_id: "rev-1",
          fields: { Body: "typed while saving" },
        }),
      )
    );
    await waitFor(() =>
      expect(onCreated).toHaveBeenCalledWith({
        id: "created-entry",
        revision_id: "rev-2",
      })
    );
  });

  it("creates a title-less entry without sending a title payload", async () => {
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "untitled-entry",
      revision_id: "untitled-revision",
    });
    const form: Form = {
      name: "Notes",
      version: 1,
      template: "# Notes\n\n## Body\n",
      fields: { Body: { type: "markdown", required: false } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const body = await screen.findByLabelText("Body");
    fireEvent.input(body, { target: { value: "hello" } });

    expect(body).toHaveValue("hello");
    // Title-less Entry: the create heading falls back to the form name and
    // never synthesizes an "Untitled" label.
    expect(screen.getByRole("heading", { name: "Notes" }))
      .toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Untitled" })).toBeNull();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(1));
    const payload = createMock.mock.calls[0][1] as Record<string, unknown>;
    expect(payload.title).toBeUndefined();
    expect(JSON.stringify(payload)).not.toContain("Untitled");
  });

  it("does not let a missing required field bypass validation for a title-less entry", async () => {
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "untitled-task-entry",
      revision_id: "untitled-task-revision",
    });
    const form: Form = {
      name: "Task",
      version: 1,
      template: "# Task\n\n## Status\n",
      fields: { Status: { type: "string", required: true } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const titleHeading = await screen.findByRole("heading", {
      name: "Task",
    });
    expect(titleHeading).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Untitled" })).toBeNull();

    const save = screen.getByRole("button", { name: "Save" });
    fireEvent.click(save);

    expect(createMock).not.toHaveBeenCalled();
    const status = screen.getByLabelText("Status");
    // Rust boundary decides saveability asynchronously; hints stay sync.
    await waitFor(() => expect(status).toHaveAttribute("aria-invalid", "true"));
    await waitFor(() => expect(document.activeElement).toBe(status));

    fireEvent.input(status, { target: { value: "Ready" } });
    fireEvent.click(save);

    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(1));
    const payload = createMock.mock.calls[0][1] as Record<string, unknown>;
    expect(payload.title).toBeUndefined();
    expect(JSON.stringify(payload)).not.toContain("Untitled");
    expect((payload.fields as Record<string, unknown>).Status).toBe("Ready");
  });

  it("blocks create until every active required field has a value", async () => {
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "created-entry",
      revision_id: "created-revision",
    });
    const form: Form = {
      name: "Task",
      version: 1,
      template: "# Task\n\n## Status\n\n## Notes\n",
      fields: {
        Status: { type: "string", required: true },
        Notes: { type: "string", required: true },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();
    expect(screen.getAllByText("This field is required.")).toHaveLength(2);

    fireEvent.click(save);

    expect(createMock).not.toHaveBeenCalled();
    // Rust is the save authority; the summary uses its classification.
    const requiredSummary = await screen.findByText("Form validation failed.");
    expect(requiredSummary).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByLabelText("Status")).toHaveAttribute(
        "aria-invalid",
        "true",
      )
    );
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByLabelText("Status"))
    );

    fireEvent.input(screen.getByLabelText("Status"), {
      target: { value: "Open" },
    });
    fireEvent.input(screen.getByLabelText("Notes"), {
      target: { value: "Details" },
    });
    fireEvent.click(save);

    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(1));
  });

  it("blocks edit until a required field is restored", async () => {
    const updateMock = entryApi.update as ReturnType<typeof vi.fn>;
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Task",
      form: "Task",
      content: "---\nform: Task\n---\n\n# Task\n\n## Status\nOpen\n",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    updateMock.mockResolvedValue({ revision_id: "rev-2" });
    const form: Form = {
      name: "Task",
      version: 1,
      template: "# Task\n\n## Status\n",
      fields: { Status: { type: "string", required: true } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [form]}
        onDeleted={vi.fn()}
      />
    ));

    const status = await screen.findByLabelText("Status");
    fireEvent.input(status, { target: { value: "" } });
    const save = screen.getByRole("button", { name: "Save" });
    fireEvent.click(save);

    expect(updateMock).not.toHaveBeenCalled();
    await waitFor(() => expect(status).toHaveAttribute("aria-invalid", "true"));
    // Wait for the async Rust validation to settle before retrying; the
    // save lock yields one revision per settled validation.
    await screen.findByText("Form validation failed.");

    fireEvent.input(status, { target: { value: "Done" } });
    fireEvent.click(save);
    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
  });

  it("blocks empty required object lists before creating an entry", async () => {
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "created-entry",
      revision_id: "created-revision",
    });
    const form: Form = {
      name: "Task",
      version: 1,
      template: "# Task\n\n## Checklist\n",
      fields: { Checklist: { type: "object_list", required: true } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const checklist = await screen.findByLabelText("Checklist");
    fireEvent.input(checklist, { target: { value: "[]" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(createMock).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(checklist).toHaveAttribute("aria-invalid", "true")
    );
    await waitFor(() => expect(document.activeElement).toBe(checklist));
  });

  it("blocks saving an empty required string list", async () => {
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "created-entry",
      revision_id: "created-revision",
    });
    const form: Form = {
      name: "Task",
      version: 1,
      template: "# Task\n\n## Items\n",
      fields: { Items: { type: "list", required: true } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    // No text conventions: zero items render no inputs, only the add
    // action. The shared Rust boundary owns required/list semantics, so
    // saving an empty required list fails validation instead of the
    // TypeScript hint inventing marker syntax.
    await screen.findByRole("button", { name: "Add item" });
    expect(screen.queryByLabelText("Items item 1")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(document.querySelector("#entry-detail-validation"))
        .not.toBeNull()
    );
    expect(createMock).not.toHaveBeenCalled();
  });

  it("focuses an empty required asset field before creating an entry", async () => {
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "created-entry",
      revision_id: "created-revision",
    });
    const form: Form = {
      name: "Contract",
      version: 1,
      template: "# Contract\n\n## Document\n",
      fields: { Document: { type: "asset_reference", required: true } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const fileInput = await screen.findByLabelText("Choose file");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(createMock).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(fileInput).toHaveAttribute("aria-invalid", "true")
    );
    expect(fileInput).toHaveAttribute(
      "aria-describedby",
      "entry-field-0-document-required",
    );
    await waitFor(() => expect(document.activeElement).toBe(fileInput));
  });

  it("does not require deprecated fields when creating an entry", async () => {
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "created-entry",
      revision_id: "created-revision",
    });
    const form: Form = {
      name: "LegacyForm",
      version: 1,
      template: "# LegacyForm\n\n## Active\n\n## Retired\n",
      fields: {
        Active: { type: "string", required: true },
        Retired: { type: "string", required: true, deprecated: true },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    expect(await screen.findByText("This field is required."))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Retired")).toBeInTheDocument();
    expect(screen.getAllByText("Optional").length).toBeGreaterThan(0);

    fireEvent.input(screen.getByLabelText("Active"), {
      target: { value: "active value" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(createMock).toHaveBeenCalled());
  });

  it("serializes a selected date from the native date input", async () => {
    const form: Form = {
      name: "Meeting",
      version: 1,
      template: "# Meeting\n\n## Date\n",
      fields: {
        Date: { type: "date", required: false },
      },
    };
    (entryApi.create as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "created-date-entry",
      revision_id: "created-date-revision",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const date = await screen.findByLabelText("Date");
    expect(date).toHaveAttribute("type", "date");
    fireEvent.input(date, { target: { value: "2026-08-03" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(entryApi.create).toHaveBeenCalled());
    const datePayload = (entryApi.create as ReturnType<typeof vi.fn>).mock
      .calls[0][1] as Record<string, unknown>;
    expect(datePayload.title).toBeUndefined();
    expect(entryApi.create).toHaveBeenCalledWith("default", {
      form: "Meeting",
      tags: [],
      fields: { Date: "2026-08-03" },
    });
  });

  it("shows field-level validation feedback and keeps the create draft", async () => {
    (entryApi.create as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error(
        'Failed to create entry: Form validation failed: [{"field":"amount","message":"Field \'amount\' has invalid type"}]',
      ),
    );
    const form: Form = {
      name: "Invoice",
      version: 1,
      template: "# Invoice\n\n## amount\n",
      fields: {
        amount: { type: "double", required: false },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const amount = await screen.findByLabelText("amount");
    fireEvent.input(amount, { target: { value: "not-a-number" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(
        screen.getByText((content) =>
          content.includes("Form validation failed")
        ),
      ).toBeInTheDocument();
    });
    expect(amount).toHaveValue("not-a-number");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("uploads an AssetReference before creating the Entry and preserves it unchanged", async () => {
    const onCreated = vi.fn();
    const uploaded = {
      asset_id: "01900000-0000-7000-8000-000000000001",
      name: "contract.pdf",
      media_type: "application/pdf",
      size_bytes: 123456,
      sha256: "a".repeat(64),
    };
    const assetUpload = (await import("~/lib/ugoite-client")).assetApi
      .upload as ReturnType<typeof vi.fn>;
    assetUpload.mockResolvedValue(uploaded);
    (entryApi.create as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "contract-entry",
      revision_id: "contract-revision",
    });
    const form: Form = {
      name: "Contract",
      version: 1,
      template: "# Contract\n\n## contract\n",
      fields: {
        contract: { type: "asset_reference", required: true },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={onCreated}
        onDeleted={vi.fn()}
      />
    ));

    const fileInput = await screen.findByLabelText("Choose file");
    fireEvent.change(fileInput, {
      target: {
        files: [new File(["pdf"], "contract.pdf", { type: "application/pdf" })],
      },
    });

    await waitFor(() => expect(assetUpload).toHaveBeenCalled());
    expect(
      screen.getByRole("button", { name: "Preview contract.pdf" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Download contract.pdf" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Uploaded; entry not saved yet")).toBeNull();
    // Filename stays out of visible row text; it lives only in the
    // preview/download accessible names and the preview dialog title.
    expect(screen.queryByText("contract.pdf")).toBeNull();
    expect(screen.queryByText(JSON.stringify(uploaded))).not
      .toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(entryApi.create).toHaveBeenCalled());
    const contractPayload = (entryApi.create as ReturnType<typeof vi.fn>).mock
      .calls[0][1] as Record<string, unknown>;
    expect(contractPayload.title).toBeUndefined();
    expect(entryApi.create).toHaveBeenCalledWith("default", {
      form: "Contract",
      tags: [],
      fields: { contract: uploaded },
    });
    expect(onCreated).toHaveBeenCalledWith({
      id: "contract-entry",
      revision_id: "contract-revision",
    });
  });

  it("rechecks required fields after async Rust validation", async () => {
    const uploaded = {
      asset_id: "01900000-0000-7000-8000-000000000002",
      name: "contract.pdf",
      media_type: "application/pdf",
      size_bytes: 123456,
      sha256: "a".repeat(64),
    };
    const validationModule = await import("~/lib/entry-validation");
    const validateSpy = vi.spyOn(validationModule, "validateEntryDraftViaWasm");
    let resolveValidation!: (
      value:
        | { ok: true; normalized: unknown }
        | {
          ok: false;
          code: string;
          message: string;
          invalidFields: string[];
          error: InstanceType<typeof UgoiteApiError>;
        },
    ) => void;
    validateSpy.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveValidation = resolve;
        }),
    );
    const assetUpload = (await import("~/lib/ugoite-client")).assetApi
      .upload as ReturnType<typeof vi.fn>;
    assetUpload.mockResolvedValue(uploaded);
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "contract-entry",
      revision_id: "contract-revision",
    });
    const form: Form = {
      name: "Contract",
      version: 1,
      template: "# Contract\n\n## Status\nReady\n\n## Document\n",
      fields: {
        Status: { type: "string", required: true },
        Document: { type: "asset_reference", required: true },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    fireEvent.change(await screen.findByLabelText("Choose file"), {
      target: {
        files: [new File(["pdf"], "contract.pdf", { type: "application/pdf" })],
      },
    });
    await waitFor(() => expect(assetUpload).toHaveBeenCalledTimes(1));
    await screen.findByRole("button", { name: "Preview contract.pdf" });

    const status = screen.getByLabelText("Status");
    const save = screen.getByRole("button", { name: "Save" });
    fireEvent.click(save);
    fireEvent.input(status, { target: { value: "" } });

    await waitFor(() => expect(validateSpy).toHaveBeenCalledTimes(1));
    resolveValidation({
      ok: false,
      code: "FORM_VALIDATION_FAILED",
      message: "Missing required field: Status (expected text)",
      invalidFields: ["Status"],
      error: new UgoiteApiError({
        kind: "entry_validation",
        message: "Missing required field: Status (expected text)",
        code: "FORM_VALIDATION_FAILED",
        detail: {
          warnings: [{
            code: "missing_field",
            field: "Status",
            expected_type: "string",
            expected_format: "text",
            reason: "required value is missing",
            message: "Missing required field: Status (expected text)",
          }],
        },
      }),
    });
    await waitFor(() => expect(createMock).not.toHaveBeenCalled());
    await waitFor(() => expect(status).toHaveAttribute("aria-invalid", "true"));
    validateSpy.mockRestore();
  });

  it("resolves normalized row_reference form ids before loading options", async () => {
    const rowReferenceOptions = searchApi
      .rowReferenceOptions as ReturnType<typeof vi.fn>;
    rowReferenceOptions.mockResolvedValue([
      { id: "project-alpha", title: "Alpha Project", form: "Project" },
    ]);
    const projectForm: Form = {
      id: "project-form-id",
      name: "Project",
      version: 1,
      template: "# Project\n\n## Summary\n",
      fields: { Summary: { type: "string", required: true } },
    };
    const taskForm: Form = {
      id: "task-form-id",
      name: "Task",
      version: 1,
      template: "# Task\n\n## Project\n",
      fields: {
        Project: {
          type: "row_reference",
          required: true,
          target_form: projectForm.id,
        },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [projectForm, taskForm]}
        createForm={() => taskForm}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const projectInput = await screen.findByLabelText("Project");
    fireEvent.input(projectInput, { target: { value: "alpha" } });

    await waitFor(() => {
      expect(rowReferenceOptions).toHaveBeenCalledWith(
        "default",
        "Project",
        "alpha",
        8,
      );
    });
  });

  it("saves after an upload completes while editing fields", async () => {
    const uploaded = {
      asset_id: "01900000-0000-7000-8000-000000000013",
      name: "pending-preview.pdf",
      media_type: "application/pdf",
      size_bytes: 123,
      sha256: "c".repeat(64),
    };
    let resolveUpload: ((reference: typeof uploaded) => void) | undefined;
    const assetUpload = (await import("~/lib/ugoite-client")).assetApi
      .upload as ReturnType<typeof vi.fn>;
    assetUpload.mockImplementation(
      () => new Promise<typeof uploaded>((resolve) => resolveUpload = resolve),
    );
    (entryApi.create as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "preview-entry",
      revision_id: "preview-revision",
    });
    const form: Form = {
      name: "PreviewAsset",
      version: 1,
      template: "# PreviewAsset\n\n## thumbnail\n",
      fields: { thumbnail: { type: "asset_reference", required: true } },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    fireEvent.change(await screen.findByLabelText("Choose file"), {
      target: {
        files: [
          new File(["pdf"], "pending-preview.pdf", {
            type: "application/pdf",
          }),
        ],
      },
    });
    await waitFor(() => expect(assetUpload).toHaveBeenCalledTimes(1));
    resolveUpload?.(uploaded);

    await screen.findByRole("button", {
      name: "Preview pending-preview.pdf",
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(entryApi.create).toHaveBeenCalledTimes(1));
    expect(entryApi.create).toHaveBeenCalledWith(
      "default",
      expect.objectContaining({
        form: "PreviewAsset",
        fields: expect.objectContaining({
          thumbnail: uploaded,
        }),
      }),
    );
  });

  it("reuses the uploaded reference when Entry save is retried", async () => {
    const uploaded = {
      asset_id: "01900000-0000-7000-8000-000000000004",
      name: "retry.txt",
      media_type: "text/plain",
      size_bytes: 4,
      sha256: "b".repeat(64),
    };
    const assetUpload = (await import("~/lib/ugoite-client")).assetApi
      .upload as ReturnType<typeof vi.fn>;
    assetUpload.mockResolvedValue(uploaded);
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock
      .mockRejectedValueOnce(new Error("save failed"))
      .mockResolvedValueOnce({
        id: "retry-entry",
        revision_id: "retry-revision",
      });
    const form: Form = {
      name: "RetryAsset",
      version: 1,
      template: "# RetryAsset\n\n## attachment\n",
      fields: { attachment: { type: "asset_reference", required: true } },
    };
    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));
    fireEvent.change(await screen.findByLabelText("Choose file"), {
      target: { files: [new File(["data"], "retry.txt")] },
    });
    await waitFor(() => expect(assetUpload).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText(/Details: save failed/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(2));
    expect(assetUpload).toHaveBeenCalledTimes(1);
  });

  it("REQ-ENTRY-1872: creates numeric and timestamp fields in one clean revision", async () => {
    const onCreated = vi.fn();
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "created-entry",
      revision_id: "created-revision",
    });
    const form: Form = {
      name: "Entry",
      version: 1,
      template: "# Entry\n\n## Body\n\n## test number\n\n## ts\n",
      fields: {
        Body: { type: "markdown", required: false },
        "test number": { type: "double", required: false },
        ts: { type: "timestamp", required: false },
      },
    };

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [form]}
        createForm={() => form}
        onCreated={onCreated}
        onDeleted={vi.fn()}
      />
    ));

    fireEvent.input(await screen.findByLabelText("test number"), {
      target: { value: "0" },
    });
    fireEvent.input(screen.getByLabelText("ts"), {
      target: { value: "2026-08-21T10:48" },
    });

    const save = screen.getByRole("button", { name: "Save" });
    fireEvent.click(save);
    fireEvent.click(save);

    await waitFor(() => expect(entryApi.create).toHaveBeenCalledTimes(1));
    expect(entryApi.update).not.toHaveBeenCalled();
    expect(entryApi.create).toHaveBeenCalledWith(
      "default",
      expect.objectContaining({
        form: "Entry",
        fields: expect.objectContaining({
          "test number": "0",
          ts: "2026-08-21T10:48",
        }),
      }),
    );
    expect(onCreated).toHaveBeenCalledWith({
      id: "created-entry",
      revision_id: "created-revision",
    });
    // Converged create action row: no unsaved/saved badge text. The
    // disabled Save action communicates the clean state.
    expect(save).toBeDisabled();
    expect(screen.queryByText("All changes saved")).not.toBeInTheDocument();
    expect(screen.queryByText("Unsaved changes")).not.toBeInTheDocument();
  });

  it("keeps nested Markdown headings out of form field values", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-nested-heading",
      title: "Nested Markdown",
      form: "Notes",
      content:
        "---\nform: Notes\n---\n\n# Nested Markdown\n\n## Notes\nhello\n\n### Details\nkeep this\n\n## Status\nopen",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-nested-heading"}
        forms={() => [
          {
            name: "Notes",
            version: 1,
            template: "# Notes\n\n## Notes\n",
            fields: {
              Notes: { type: "markdown", required: false },
              Status: { type: "string", required: false },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    const notes = await screen.findByLabelText("Notes");
    expect(notes).toHaveValue("hello");

    fireEvent.input(notes, { target: { value: "updated" } });

    const source = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    expect((source as HTMLTextAreaElement).value).toContain(
      "## Notes\nupdated\n\n### Details\nkeep this",
    );
    expect(
      ((source as HTMLTextAreaElement).value.match(/### Details/g) || [])
        .length,
    ).toBe(1);
  });

  it("REQ-FE-052: preserves timestamp values in form-first controls", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-timestamps",
      title: "Timestamp Entry",
      form: "Event",
      content:
        "---\nform: Event\n---\n\n# Timestamp Entry\n\n## Amount\n12.5\n\n## Started\n2026-07-18T12:34:56Z\n\n## Observed\n2026-07-18T21:34:56+09:00\n\n## Precise\n2026-07-18T12:34:56.123456789Z\n\n## Date\n2026-07-18\n\n## Time\n12:",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-timestamps"}
        forms={() => [
          {
            name: "Event",
            version: 1,
            template: "# Event\n",
            fields: {
              Amount: { type: "double", required: false },
              Started: { type: "timestamp", required: false },
              Observed: { type: "timestamp_tz", required: false },
              Precise: { type: "timestamp_ns", required: false },
              Date: { type: "date", required: false },
              Time: { type: "time", required: false },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    const started = await screen.findByLabelText("Started");
    expect(started).toHaveAttribute("type", "text");
    expect(started).toHaveValue("2026-07-18T12:34:56Z");
    expect(started).not.toHaveAttribute("step");

    const amount = screen.getByLabelText("Amount");
    expect(amount).toHaveAttribute("type", "text");
    expect(amount).toHaveAttribute("inputmode", "decimal");
    fireEvent.input(amount, { target: { value: "12." } });
    expect(amount).toHaveValue("12.");

    const observed = screen.getByLabelText("Observed");
    expect(observed).toHaveAttribute("type", "text");
    expect(observed).toHaveValue("2026-07-18T21:34:56+09:00");

    const precise = screen.getByLabelText("Precise");
    expect(precise).toHaveAttribute("type", "text");
    expect(precise).toHaveValue("2026-07-18T12:34:56.123456789Z");

    const date = screen.getByLabelText("Date");
    expect(date).toHaveAttribute("type", "date");
    expect(date).toHaveValue("2026-07-18");

    const time = screen.getByLabelText("Time");
    expect(time).toHaveAttribute("type", "text");
    expect(time).toHaveValue("12:");
  });

  it("REQ-FE-052: explains forms that have no structured fields", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-2",
      title: "Scratch Note",
      form: "Empty",
      content: "---\nform: Empty\n---\n\n# Scratch Note",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-2"}
        forms={() => [
          {
            name: "Empty",
            version: 1,
            template: "# Empty\n",
            fields: {},
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    expect(await screen.findByText("This form has no structured fields."))
      .toBeInTheDocument();
    // Title-less Entry: no Entry-level title control; the heading shows the
    // trimmed legacy title.
    expect(screen.queryByLabelText("Title")).toBeNull();
    expect(screen.getByRole("heading", { name: "Scratch Note" }))
      .toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open source editor" }))
      .toBeInTheDocument();
  });

  it("REQ-FE-052: tolerates form data without a fields map", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-3",
      title: "Broken Note",
      form: "Broken",
      content: "---\nform: Broken\n---\n\n# Broken Note",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-3"}
        forms={() => [
          {
            name: "Broken",
            version: 1,
            template: "# Broken\n",
          } as unknown as Form,
        ]}
        onDeleted={vi.fn()}
      />
    ));

    expect(await screen.findByText("This form has no structured fields."))
      .toBeInTheDocument();
    expect(screen.queryByLabelText("Title")).toBeNull();
    expect(screen.getByRole("heading", { name: "Broken Note" }))
      .toBeInTheDocument();
  });

  it("REQ-FE-053: keeps type and additional-content warnings next to the form", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-4",
      title: "Task Entry",
      form: "Task",
      content:
        "---\nform: Task\n---\n\n# Task Entry\n\n## Summary\nhello\n\n## Done\nmaybe\n\n## Extra\nvalue",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-4"}
        forms={() => [
          {
            name: "Task",
            version: 1,
            template: "# Task\n\n## Summary\n\n## Done\n",
            fields: {
              Summary: { type: "string", required: true },
              Done: { type: "boolean", required: false },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    expect(await screen.findByLabelText("Summary")).toHaveValue("hello");
    expect(screen.getByLabelText("Done")).toHaveAttribute("type", "text");
    expect(screen.getByLabelText("Done")).toHaveValue("maybe");
    expect(
      screen.getByText(
        "Done: Use true/false, yes/no, on/off, or 1/0 for boolean fields.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Additional Markdown content")).toBeInTheDocument();
    expect(screen.getByText("Extra")).toBeInTheDocument();
  });

  it("REQ-FE-053: renders the form-first editor in Japanese", async () => {
    setLocale("ja");
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-ja",
      title: "タスク",
      form: "Task",
      content: "---\nform: Task\n---\n\n# タスク\n\n## Summary\nhello",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-ja"}
        forms={() => [
          {
            name: "Task",
            version: 1,
            template: "# Task\n\n## Summary\n",
            fields: {
              Summary: { type: "string", required: true },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    expect(await screen.findByLabelText("Summary")).toHaveValue("hello");
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByText("見慣れたフォーム項目からエントリを編集します。"))
      .not.toBeInTheDocument();
    expect(screen.queryByText("項目")).not.toBeInTheDocument();
  });

  it("REQ-FE-033: entry detail returns to its Form workspace", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: "Notes",
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    const backLink = await screen.findByRole("link", {
      name: "Back to Form",
    });

    expect(backLink).toHaveAttribute(
      "href",
      "/spaces/default/forms?form=Notes",
    );
  });
  it("REQ-FE-038: renders form validation warnings", async () => {
    setLocale("ja");
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: "Meeting",
      content: "---\nform: Meeting\n---\n# Test Entry\n\n## Date\n",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "invalid_arguments",
        code: "FORM_VALIDATION_FAILED",
        operation: "entry.update",
        status: 422,
        message: "Entry form validation failed",
        detail: {
          warnings: [{
            field: "Date",
            message: "Missing required field: Date",
          }],
        },
      }),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

    const textarea = await screen.findByPlaceholderText(
      "Markdown を入力...",
    );
    fireEvent.input(textarea, { target: { value: "Updated content" } });

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(screen.getByText("フォームの入力内容を確認してください。"))
        .toBeInTheDocument();
      expect(screen.getByText("Missing required field: Date"))
        .toBeInTheDocument();
    });
  });

  it("shows error message when entry load fails", async () => {
    setLocale("ja");
    (entryApi.get as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "not_found",
        code: "ENTRY_NOT_FOUND",
        operation: "entry.get",
        status: 404,
        message: "Entry not found",
      }),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "missing-entry"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => {
      expect(
        screen.getByText(
          "エントリーが見つかりません。（詳細: Entry not found）",
        ),
      )
        .toBeInTheDocument();
      expect(screen.getByText("スペース: default / エントリID: missing-entry"))
        .toBeInTheDocument();
    });
  });

  it("saves successfully and marks editor clean", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.update as ReturnType<typeof vi.fn>).mockResolvedValue({
      revision_id: "rev-2",
    });
    const onAfterSave = vi.fn();

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
        onAfterSave={onAfterSave}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "Updated content" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(entryApi.update).toHaveBeenCalled();
      expect(onAfterSave).toHaveBeenCalled();
    });
    expect(entryApi.update).toHaveBeenCalledWith("default", "entry-1", {
      markdown: "Updated content",
      parent_revision_id: "rev-1",
    });
  });

  it("REQ-FE-013: sends current Markdown and parent revision to the server", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.update as ReturnType<typeof vi.fn>).mockResolvedValue({
      revision_id: "rev-2",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "# Persisted Markdown" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(entryApi.update).toHaveBeenCalledWith("default", "entry-1", {
        markdown: "# Persisted Markdown",
        parent_revision_id: "rev-1",
      });
    });
  });

  it("keeps edits made during a save marked as unsaved", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    let finishSave: ((value: { revision_id: string }) => void) | undefined;
    (entryApi.update as ReturnType<typeof vi.fn>).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishSave = resolve;
        }),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "First edit" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(entryApi.update).toHaveBeenCalledWith("default", "entry-1", {
        markdown: "First edit",
        parent_revision_id: "rev-1",
      });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Saving...");

    fireEvent.input(textarea, { target: { value: "Second edit" } });
    finishSave?.({ revision_id: "rev-2" });

    await waitFor(() => {
      // PR4: dirtiness surfaces through the strong enabled save tool; the
      // permanent unsaved chip is gone.
      const pendingSave = screen.getByRole("button", { name: "Save" });
      expect(pendingSave).toBeEnabled();
      expect(pendingSave.classList.contains("ui-entry-tool-primary")).toBe(
        true,
      );
    });
  });

  it("keeps the stable Save label with a spinner and blocks double-submit", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    let finishSave: ((value: { revision_id: string }) => void) | undefined;
    const updateMock = entryApi.update as ReturnType<typeof vi.fn>;
    updateMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishSave = resolve;
        }),
    );

    const { container } = render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "# Edited" } });
    const save = screen.getByRole("button", { name: "Save" });
    fireEvent.click(save);

    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    // Busy Save retains its accessible name, disables, and shows a spinner
    // instead of swapping to Saving...; previous content stays visible.
    const busySave = screen.getByRole("button", { name: "Save" });
    expect(busySave).toBeDisabled();
    expect(busySave).toHaveAttribute("aria-busy", "true");
    expect(busySave.textContent).toContain("Save");
    expect(busySave.textContent).not.toContain("Saving...");
    expect(container.querySelector(".btnSpinner")).toBeInTheDocument();
    expect(textarea).toHaveValue("# Edited");

    // No double-submit while busy.
    fireEvent.click(busySave);
    expect(updateMock).toHaveBeenCalledTimes(1);

    finishSave?.({ revision_id: "rev-2" });
    await waitFor(() => {
      const saved = screen.getByRole("button", { name: "Save" });
      expect(saved).not.toHaveAttribute("aria-busy");
      expect(saved.closest("button")?.querySelector(".btnSpinner")).toBeNull();
    });
  });

  it("shows unknown fields warning from save error", async () => {
    setLocale("ja");
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "invalid_arguments",
        code: "UNKNOWN_FORM_FIELDS",
        operation: "entry.update",
        status: 422,
        message: "Entry contains unknown form fields",
        detail: { fields: ["extraField1", "extraField2"] },
      }),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

    const textarea = await screen.findByPlaceholderText(
      "Markdown を入力...",
    );
    fireEvent.input(textarea, { target: { value: "Updated content" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(screen.getByText("フォームにないフィールドがあります。"))
        .toBeInTheDocument();
    });
  });

  it("does not parse backend validation prefixes as a second error protocol", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "invalid_arguments",
        code: "INVALID_INPUT",
        operation: "entry.update",
        status: 422,
        message: "Server validation failed",
        detail: "not-valid-json",
      }),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "Updated content" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(screen.getByText(/The request is invalid.*not-valid-json/))
        .toBeInTheDocument();
    });
  });

  it("shows conflict message on generic save error", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error("Server unavailable"),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "Updated content" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(
        screen.getByText(
          "Failed to save the entry. (Details: Server unavailable)",
        ),
      ).toBeInTheDocument();
    });
  });

  it("REQ-FE-009: shows refresh guidance on revision conflict", async () => {
    setLocale("ja");
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
      new RevisionConflictError("Revision conflict", "server-rev"),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

    const textarea = await screen.findByPlaceholderText(
      "Markdown を入力...",
    );
    fireEvent.input(textarea, { target: { value: "Updated content" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(
        screen.getByText("保存する前にエントリーが変更されました。"),
      ).toBeInTheDocument();
    });
  });

  it("localizes delete failures while preserving the typed API boundary", async () => {
    setLocale("ja");
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.delete as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UgoiteApiError({
        kind: "not_found",
        code: "ENTRY_NOT_FOUND",
        operation: "entry.delete",
        status: 404,
        message: "Entry not found",
      }),
    );
    vi.stubGlobal("confirm", () => true);
    const alertMock = vi.fn();
    vi.stubGlobal("alert", alertMock);

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => screen.getByRole("button", { name: "エントリを削除" }));
    fireEvent.click(screen.getByRole("button", { name: "エントリを削除" }));

    await waitFor(() => {
      expect(alertMock).toHaveBeenCalledWith(
        "エントリーが見つかりません。（詳細: Entry not found）",
      );
    });
    vi.unstubAllGlobals();
  });

  it("PR4: keeps a single one-row action bar with save/history/info/delete", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-pr3",
      title: "PR3 Entry",
      form: "Meeting",
      content:
        "---\nform: Meeting\n---\n\n# PR3 Entry\n\n## Summary\nhello\n\n## Notes\n**review**",
      revision_id: "rev-pr3",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-pr3"}
        forms={() => [
          {
            name: "Meeting",
            version: 1,
            template: "# Meeting\n\n## Summary\n\n## Notes\n",
            fields: {
              Summary: { type: "string", required: false },
              Notes: { type: "markdown", required: false },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    await screen.findByLabelText("Summary");

    // Single toolbar, four tools: save rides in the bar (weak and disabled
    // while clean), then history/info/delete. No separate header save area,
    // no refresh tool.
    const bar = document.querySelector(".actionbar.compact-actions");
    expect(bar).not.toBeNull();
    expect(bar?.getAttribute("role")).toBe("toolbar");
    expect(bar?.querySelectorAll(".tool")).toHaveLength(4);
    for (const short of ["Save", "History", "Info", "Delete"]) {
      expect(bar?.textContent).toContain(short);
    }
    expect(bar?.textContent).not.toContain("Refresh");
    const save = screen.getByRole("button", { name: "Save" });
    expect(bar?.contains(save)).toBe(true);
    expect(save).toBeDisabled();
    expect(save.classList.contains("ui-entry-tool-primary")).toBe(false);
    expect(screen.getByRole("link", { name: /History & recovery/ }))
      .toHaveAttribute(
        "href",
        "/spaces/default/entries/entry-pr3/history",
      );
    expect(screen.getByRole("link", { name: "Info" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete entry" }))
      .toBeInTheDocument();

    // Header row: shared back link + title + form chip, in order. Save lives
    // in the bar below the header, not in a separate header save area.
    const page = document.querySelector(".ui-entry-page")!;
    const header = page.querySelector(".ui-entry-header")!;
    const backLink = header.querySelector('a[href*="/forms"]')!;
    const title = header.querySelector("h1")!;
    const chip = header.querySelector(".ui-pill")!;
    expect(header.contains(backLink)).toBe(true);
    expect(title).toHaveTextContent("PR3 Entry");
    expect(chip).toHaveTextContent("Meeting");
    expect(header.querySelector(".ui-entry-save-area")).toBeNull();
    expect(
      backLink.compareDocumentPosition(title) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      header.compareDocumentPosition(save) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    // No permanent saved chip in the detail view: no live region while idle.
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByText("All changes saved")).not.toBeInTheDocument();

    // Dirty editing turns save strong; the markdown type label stays out of
    // normal field rendering (the textarea control expresses it).
    const notes = screen.getByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "**review**!" } });
    await waitFor(() => expect(save).toBeEnabled());
    expect(save.classList.contains("ui-entry-tool-primary")).toBe(true);
    expect(screen.queryByText("markdown")).not.toBeInTheDocument();

    // Shared entry fields with consistent spacing hooks.
    const fields = document.querySelector(".form.entry-fields")!;
    expect(fields).not.toBeNull();
    // Title-less Entry: no Entry-level title row, only the Form fields.
    expect(fields.querySelectorAll(".field")).toHaveLength(2);
    expect(screen.getByLabelText("Notes")).toHaveValue("**review**!");
  });

  it("PR4: announces saving without layout shift and toasts success transiently", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    let finishSave: ((value: { revision_id: string }) => void) | undefined;
    (entryApi.update as ReturnType<typeof vi.fn>).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishSave = resolve;
        }),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    const textarea = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "# Edited" } });
    const save = screen.getByRole("button", { name: "Save" });
    // Same box before and during the save: the label never swaps text.
    const before = save.getBoundingClientRect();
    fireEvent.click(save);

    await waitFor(() => expect(entryApi.update).toHaveBeenCalledTimes(1));
    const busySave = screen.getByRole("button", { name: "Save" });
    expect(busySave).toHaveAttribute("aria-busy", "true");
    expect(busySave.textContent).toContain("Save");
    expect(busySave.textContent).not.toContain("Saving...");
    const during = busySave.getBoundingClientRect();
    expect(during.width).toBe(before.width);
    expect(during.height).toBe(before.height);
    // The accessible announcement carries the saving state.
    expect(screen.getByRole("status")).toHaveTextContent("Saving...");

    finishSave?.({ revision_id: "rev-2" });
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "All changes saved",
      )
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("calls onDeleted after successful delete", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    (entryApi.delete as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);
    const onDeleted = vi.fn();
    vi.stubGlobal("confirm", () => true);

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={onDeleted}
      />
    ));

    // Wait for entry header to appear (entry is loaded)
    await waitFor(() => screen.getByRole("button", { name: "Delete entry" }));

    fireEvent.click(screen.getByRole("button", { name: "Delete entry" }));

    await waitFor(() => {
      expect(entryApi.delete).toHaveBeenCalledWith("default", "entry-1");
      expect(onDeleted).toHaveBeenCalled();
    });

    vi.unstubAllGlobals();
  });
});
