// REQ-FE-038: Form validation feedback in editor
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { EntryDetailPane } from "./EntryDetailPane";
import { entryApi, RevisionConflictError } from "~/lib/ugoite-client";
import { assetApi } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children}
    </a>
  ),
}));

vi.mock("~/lib/ugoite-client", () => {
  class RevisionConflictError extends Error {}
  return {
    assetApi: {
      list: vi.fn(),
      upload: vi.fn(),
    },
    entryApi: {
      get: vi.fn(),
      create: vi.fn(),
      update: vi.fn(),
      delete: vi.fn(),
    },
    searchApi: {
      rowReferenceOptions: vi.fn(),
    },
    RevisionConflictError,
  };
});

describe("EntryDetailPane", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    (assetApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
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
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));

    const source = await screen.findByPlaceholderText(
      "Start writing in Markdown...",
    );
    expect((source as HTMLTextAreaElement).value).toContain(
      "## Date\n2026-07-16",
    );
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
      template: "# Meeting\n\n## Summary\n\n## Notes\n\n## Items\n",
      fields: {
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

    const title = await screen.findByLabelText("Title");
    expect(title).toHaveValue("Meeting");
    fireEvent.input(title, { target: { value: "Planning " } });
    expect(title).toHaveValue("Planning ");

    const summary = await screen.findByLabelText("Summary");
    fireEvent.input(summary, { target: { value: "Project " } });
    expect(summary).toHaveValue("Project ");

    const notes = await screen.findByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "Details \n" } });
    expect(notes).toHaveValue("Details \n");

    const items = await screen.findByLabelText("Items");
    fireEvent.input(items, { target: { value: "one\ntwo\n" } });
    expect(items).toHaveValue("one\ntwo\n");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(entryApi.create).toHaveBeenCalled());
    expect(entryApi.create).toHaveBeenCalledWith("default", {
      markdown:
        "---\nform: Meeting\n---\n\n# Planning \n\n## Summary\nProject \n\n## Notes\nDetails \n\n\n## Items\none\ntwo\n\n",
    });
    expect(onCreated).toHaveBeenCalledWith({
      id: "created-entry",
      revision_id: "created-revision",
    });
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
    expect(entryApi.create).toHaveBeenCalledWith("default", {
      markdown: expect.stringContaining("## test number\n0"),
    });
    expect(createMock.mock.calls[0][1].markdown).toContain(
      "## ts\n2026-08-21T10:48",
    );
    expect(onCreated).toHaveBeenCalledWith({
      id: "created-entry",
      revision_id: "created-revision",
    });
    expect(screen.getByText("All changes saved")).toBeInTheDocument();
    expect(save).toBeDisabled();
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
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));

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
        "---\nform: Event\n---\n\n# Timestamp Entry\n\n## Started\n2026-07-18T12:34:56Z\n\n## Observed\n2026-07-18T21:34:56+09:00\n\n## Precise\n2026-07-18T12:34:56.123456789Z",
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
              Started: { type: "timestamp", required: false },
              Observed: { type: "timestamp_tz", required: false },
              Precise: { type: "timestamp_ns", required: false },
            },
          },
        ]}
        onDeleted={vi.fn()}
      />
    ));

    const started = await screen.findByLabelText("Started");
    expect(started).toHaveAttribute("type", "datetime-local");
    expect(started).toHaveValue("2026-07-18T12:34:56.000");
    expect(started).toHaveAttribute("step", "any");

    const observed = screen.getByLabelText("Observed");
    expect(observed).toHaveAttribute("type", "text");
    expect(observed).toHaveValue("2026-07-18T21:34:56+09:00");

    const precise = screen.getByLabelText("Precise");
    expect(precise).toHaveAttribute("type", "text");
    expect(precise).toHaveValue("2026-07-18T12:34:56.123456789Z");
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
    expect(screen.getByLabelText("Title")).toHaveValue("Scratch Note");
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
    expect(screen.getByLabelText("Title")).toHaveValue("Broken Note");
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

    expect(await screen.findByRole("tab", { name: "項目" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("見慣れたフォーム項目からエントリを編集します。"))
      .toBeInTheDocument();
    expect(screen.getByLabelText("Summary")).toHaveValue("hello");
    expect(screen.queryByRole("tab", { name: "Fields" })).not
      .toBeInTheDocument();
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
      new Error(
        'Form validation failed: [{"field":"Date","message":"Missing required field: Date"}]',
      ),
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
      expect(screen.getByText("Form validation failed")).toBeInTheDocument();
      expect(screen.getByText("Missing required field: Date"))
        .toBeInTheDocument();
    });
  });

  it("shows error message when entry load fails", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error("Network error"),
    );

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "missing-entry"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => {
      expect(screen.getByText("Network error")).toBeInTheDocument();
    });
  });

  it("calls assetApi.upload when file is uploaded", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "entry-1",
      title: "Test Entry",
      form: null,
      content: "# Test Entry",
      revision_id: "rev-1",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    const mockAsset = {
      id: "asset-1",
      name: "file.txt",
      path: "/path/file.txt",
    };
    (assetApi.upload as ReturnType<typeof vi.fn>).mockResolvedValue(mockAsset);
    (assetApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([mockAsset]);

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        onDeleted={vi.fn()}
      />
    ));

    await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

    // Wait for the file input to be present (after entry loads)
    const fileInput = await waitFor(() => {
      const el = document.querySelector(
        'input[type="file"]',
      ) as HTMLInputElement;
      if (!el) throw new Error("file input not found");
      return el;
    });

    const file = new File(["content"], "file.txt", { type: "text/plain" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() => {
      expect(assetApi.upload).toHaveBeenCalledWith("default", file);
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

    fireEvent.input(textarea, { target: { value: "Second edit" } });
    finishSave?.({ revision_id: "rev-2" });

    await waitFor(() => {
      expect(screen.getByText("Unsaved changes")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    });
  });

  it("shows unknown fields warning from save error", async () => {
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
      new Error("Unknown form fields: extraField1, extraField2"),
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
      expect(screen.getByText("Unknown form fields")).toBeInTheDocument();
    });
  });

  it("handles malformed JSON in validation error message", async () => {
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
      new Error("Form validation failed: not-valid-json"),
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
      expect(screen.getByText("Form validation failed")).toBeInTheDocument();
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
      expect(screen.getByText("Server unavailable")).toBeInTheDocument();
    });
  });

  it("REQ-FE-009: shows refresh guidance on revision conflict", async () => {
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
      "Start writing in Markdown...",
    );
    fireEvent.input(textarea, { target: { value: "Updated content" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(
        screen.getByText(
          "This entry was modified elsewhere. Your draft is still in the editor; refresh to load the latest version.",
        ),
      ).toBeInTheDocument();
    });
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
