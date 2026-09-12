// PR4: source compatibility rides the Rust bridge, not TS helpers.
import "@testing-library/jest-dom/vitest";
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { EntryDetailPane } from "./EntryDetailPane";
import { entryApi } from "~/lib/ugoite-client";
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
});
