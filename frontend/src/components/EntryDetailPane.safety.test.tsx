// PR-06 safety/recovery: destructive confirmations and 409 draft preservation.
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { EntryDetailPane } from "./EntryDetailPane";
import { entryApi, RevisionConflictError } from "~/lib/ugoite-client";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";

let capturedBeforeLeave:
  | ((event: {
    defaultPrevented: boolean;
    preventDefault: () => void;
    retry: (force?: boolean) => void;
  }) => void)
  | undefined;

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
  useBeforeLeave: (handler: typeof capturedBeforeLeave) => {
    capturedBeforeLeave = handler;
  },
}));

vi.mock("~/lib/ugoite-client", () => {
  class RevisionConflictError extends Error {
    currentRevisionId?: string;
    apiError?: unknown;
    constructor(
      message: string,
      currentRevisionId?: string,
      apiError?: unknown,
    ) {
      super(message);
      this.name = "RevisionConflictError";
      this.currentRevisionId = currentRevisionId;
      this.apiError = apiError;
    }
  }
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
    RevisionConflictError,
  };
});

const notesForm: Form = {
  id: "00000000-0000-7000-8000-000000000001",
  name: "Notes",
  version: 1,
  template: "# Notes\n\n## Notes\n",
  fields: {
    Notes: { id: 100, type: "string", required: false },
  },
};

const storedEntry = (overrides: Record<string, unknown> = {}) => ({
  id: "entry-1",
  title: "Team notes",
  form: "Notes",
  fields: { Notes: "hello" },
  revision_id: "rev-1",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...overrides,
});

describe("EntryDetailPane safety/recovery", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    capturedBeforeLeave = undefined;
    setLocale("en");
  });

  it("cancelling the delete dialog sends nothing and keeps the draft", async () => {
    (entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue(
      storedEntry(),
    );
    const onDeleted = vi.fn();

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [notesForm]}
        onDeleted={onDeleted}
      />
    ));

    const notes = await screen.findByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "local edit" } });

    fireEvent.click(screen.getByRole("button", { name: "Delete entry" }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveAccessibleName("Delete this entry?");
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toHaveFocus();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Cancel" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(entryApi.delete).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
    // The draft survives the cancelled delete.
    expect(screen.getByLabelText("Notes")).toHaveValue("local edit");
  });

  it("keeps the user draft on 409, shows the server revision, and re-saves on the adopted base", async () => {
    const getMock = entryApi.get as ReturnType<typeof vi.fn>;
    getMock.mockResolvedValueOnce(storedEntry()).mockResolvedValueOnce(
      storedEntry({
        fields: { Notes: "teammate version" },
        revision_id: "server-rev",
      }),
    );
    const updateMock = entryApi.update as ReturnType<typeof vi.fn>;
    updateMock.mockRejectedValueOnce(
      new RevisionConflictError("Revision conflict", "server-rev"),
    );
    updateMock.mockResolvedValue({ id: "entry-1", revision_id: "rev-3" });

    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        entryId={() => "entry-1"}
        forms={() => [notesForm]}
        onDeleted={vi.fn()}
      />
    ));

    const notes = await screen.findByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "my draft edit" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    // No Reload-destroys-draft: the heading, server revision, and the exact
    // typed draft all stay visible.
    const conflict = await screen.findByText(
      "Someone else saved first — your draft is kept",
    );
    expect(conflict).toBeInTheDocument();
    expect(
      screen.getByText("Current server revision: server-rev"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Notes")).toHaveValue("my draft edit");

    // Side-by-side review: latest is read-only text, local stays editable.
    fireEvent.click(
      screen.getByRole("button", { name: "Show latest saved version" }),
    );
    const latestSection = await screen.findByLabelText(
      "Latest saved version (read-only)",
    );
    expect(latestSection).toHaveTextContent("teammate version");
    expect(
      within(latestSection).queryByRole("textbox"),
    ).toBeNull();
    const localSection = screen.getByLabelText(
      "Your draft (keeps your edits)",
    );
    expect(localSection).toHaveTextContent("my draft edit");

    // Adopting the base re-points the next save; the draft is untouched and
    // no auto-merge rewrites either side.
    fireEvent.click(
      screen.getByRole("button", {
        name: "Continue editing on the latest revision",
      }),
    );
    expect(screen.getByLabelText("Notes")).toHaveValue("my draft edit");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(2));
    const retryPayload = updateMock.mock.calls[1][2] as Record<
      string,
      unknown
    >;
    expect(retryPayload.parent_revision_id).toBe("server-rev");
    await waitFor(() =>
      expect(
        screen.queryByText("Someone else saved first — your draft is kept"),
      ).not.toBeInTheDocument()
    );
  });

  it("confirms unsaved-work navigation through the shared dialog, never window.confirm", async () => {
    // The navigation guard covers authoring sessions (create flow): typing
    // marks dirty work, and leaving asks through the shared dialog.
    render(() => (
      <EntryDetailPane
        spaceId={() => "default"}
        forms={() => [notesForm]}
        createForm={() => notesForm}
        onCreated={vi.fn()}
        onDeleted={vi.fn()}
      />
    ));

    const notes = await screen.findByLabelText("Notes");
    fireEvent.input(notes, { target: { value: "unsaved draft" } });
    expect(capturedBeforeLeave).toBeDefined();

    let prevented = false;
    let retried = false;
    capturedBeforeLeave!({
      defaultPrevented: false,
      preventDefault: () => {
        prevented = true;
      },
      retry: () => {
        retried = true;
      },
    });

    expect(prevented).toBe(true);
    expect(retried).toBe(false);
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveAccessibleName("Leave without saving?");
    expect(dialog).toHaveAccessibleDescription(
      "You have unsaved Entry work. Leave and discard it?",
    );

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Discard draft changes" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(retried).toBe(true);
  });
});
