// PR4: source compatibility rides the Rust bridge, not TS helpers.
import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { EntryDetailPane } from "./EntryDetailPane";
import { entryApi, searchApi } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
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

    fireEvent.click(await screen.findByRole("button", { name: "Source" }));
    const source = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    fireEvent.input(source, {
      target: {
        value: "---\nform: Note\n---\n# Note\n\n## Body\nedited via source\n",
      },
    });

    // Fields view reconciles through the Rust bridge.
    fireEvent.click(screen.getByRole("tab", { name: "Fields" }));
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

    fireEvent.click(await screen.findByRole("button", { name: "Source" }));
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
    fireEvent.click(screen.getByRole("tab", { name: "Fields" }));
    await waitFor(() =>
      expect(screen.getByLabelText("Body")).toHaveValue("bridge wins")
    );
    parseSpy.mockRestore();
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
