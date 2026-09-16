// PR4: source compatibility rides the Rust bridge, not TS helpers.
import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { EntryDetailPane as ActualEntryDetailPane } from "./EntryDetailPane";
import { entryApi, searchApi } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";
import {
  clearCreateEntryDraftSession,
  createEntryDraftSessionKey,
} from "~/lib/create-entry-draft-session";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
  useBeforeLeave: () => undefined,
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: {
    get: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    delete: vi.fn(),
  },
  assetApi: { upload: vi.fn(), read: vi.fn() },
  searchApi: { rowReferenceOptions: vi.fn() },
  RevisionConflictError: class RevisionConflictError extends Error {},
}));

// The mocked Form responses are normalized here to the stable identities a
// real server read supplies. The production adapter must never do this.
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

const form: Form = {
  name: "Note",
  version: 1,
  template: "# Note\n\n## Body\n",
  fields: {
    Body: { type: "string", required: true },
    Done: { type: "boolean", required: false },
  },
};

describe("EntryDetailPane source compat bridge", () => {
  beforeEach(() => {
    clearCreateEntryDraftSession(createEntryDraftSessionKey("default"));
  });

  it("returns source-edited legacy Markdown to the structured draft", async () => {
    setLocale("en");
    vi.resetAllMocks();
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Note",
      form: "Note",
      content: "---\nform: Note\n---\n# Note\n\n## Body\nhello\n",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [form]}
        onDeleted={vi.fn()}
      />
    ));

    fireEvent.input(await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    ), {
      target: {
        value: "---\nform: Note\n---\n# Note\n\n## Body\nedited via source\n",
      },
    });

    // Fields stay visible and reconcile through the Rust bridge.
    await waitFor(() =>
      expect(screen.getByLabelText("Body")).toHaveValue("edited via source")
    );
  });

  it("does not use TS Markdown helpers as Entry semantic authority", async () => {
    setLocale("en");
    vi.resetAllMocks();
    const entryInput = await import("~/lib/entry-input");
    const parseSpy = vi.spyOn(entryInput, "parseMarkdownToStructuredDraft");
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Note",
      form: "Note",
      content: "---\nform: Note\n---\n# Note\n\n## Body\nhello\n",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [form]}
        onDeleted={vi.fn()}
      />
    ));

    const source = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    // TS helper is not semantic authority: even if it throws, source ingress
    // through the Rust bridge still reconciles.
    parseSpy.mockImplementation(() => {
      throw new Error("TS helper must not decide semantics");
    });
    fireEvent.input(source, {
      target: {
        value: "---\nform: Note\n---\n# Note\n\n## Body\nbridge wins\n",
      },
    });
    await waitFor(() =>
      expect(screen.getByLabelText("Body")).toHaveValue("bridge wins")
    );
    parseSpy.mockRestore();
  });

  it("blocks lossy source saves until the canonical version is accepted", async () => {
    setLocale("en");
    vi.resetAllMocks();
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Note",
      form: "Note",
      content: "---\nform: Note\n---\n# Note\n\n## Body\nhello\n",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    const updateMock = entryApi.update as ReturnType<typeof vi.fn>;
    updateMock.mockResolvedValue({ revision_id: "rev-2" });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [form]}
        onDeleted={vi.fn()}
      />
    ));

    const lossy = "---\nform: Note\n---\n# Note\n\nPreamble\n\n## Body\nkept\n";
    fireEvent.input(await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    ), { target: { value: lossy } });

    await waitFor(() => {
      expect(screen.getByText("Review Markdown conversion before saving"))
        .toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(updateMock).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", { name: "Use canonical version" }),
    );
    await waitFor(() => {
      expect(screen.queryByText("Review Markdown conversion before saving"))
        .not.toBeInTheDocument();
      expect(
        screen.getByPlaceholderText("Start writing in Markdown..."),
      ).not.toHaveValue(lossy);
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    expect(updateMock).toHaveBeenCalledWith("default", "entry-1", {
      form: "Note",
      title: "Note",
      tags: [],
      fields: { Body: "kept" },
      parent_revision_id: "rev-1",
    });
  });

  it("auto-opens the Advanced source disclosure while saves are blocked", async () => {
    setLocale("en");
    vi.resetAllMocks();
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Note",
      form: "Note",
      content: "---\nform: Note\n---\n# Note\n\n## Body\nhello\n",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });

    const { container } = render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [form]}
        onDeleted={vi.fn()}
      />
    ));

    const lossy = "---\nform: Note\n---\n# Note\n\nPreamble\n\n## Body\nkept\n";
    fireEvent.input(await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    ), { target: { value: lossy } });

    // Save stays blocked and the blocking source stays visible for review.
    await waitFor(() => {
      expect(screen.getByText("Review Markdown conversion before saving"))
        .toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    });
    await waitFor(() => {
      const disclosure = container.querySelector(
        "details.ui-entry-source-disclosure",
      ) as HTMLDetailsElement | null;
      expect(disclosure?.open).toBe(true);
    });
  });

  it("keeps the saved row_reference ID when the display label changes", async () => {
    setLocale("en");
    vi.resetAllMocks();
    const rowReferenceOptions = searchApi
      .rowReferenceOptions as ReturnType<typeof vi.fn>;
    rowReferenceOptions.mockResolvedValue([
      { id: "project-alpha", title: "Alpha Project", form: "Project" },
    ]);
    const createMock = entryApi.create as ReturnType<typeof vi.fn>;
    createMock.mockResolvedValue({
      id: "task-entry",
      revision_id: "rev-1",
    });
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
    await waitFor(() => expect(rowReferenceOptions).toHaveBeenCalled());
    fireEvent.click(await screen.findByText("Alpha Project"));

    // Label changes upstream; the saved draft value stays the stable ID.
    rowReferenceOptions.mockResolvedValue([
      {
        id: "project-alpha",
        title: "Alpha Project (renamed)",
        form: "Project",
      },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(1));
    const payload = createMock.mock.calls[0][1] as {
      fields: Record<string, unknown>;
    };
    expect(payload.fields.Project).toBe("project-alpha");
  });
});
