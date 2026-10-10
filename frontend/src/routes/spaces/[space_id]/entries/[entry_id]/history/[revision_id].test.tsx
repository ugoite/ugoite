import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { entryApi, spaceApi } from "~/lib/ugoite-client";
import SpaceEntryRevisionRoute from "./[revision_id]";
import { expectBackLinkAtHeaderStart } from "~/test/back-link-placement";

const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
  useNavigate: () => navigate,
  useParams: () => ({
    space_id: "default",
    entry_id: "entry-1",
    revision_id: "rev-old",
  }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: {
    get: vi.fn(),
    getRevision: vi.fn(),
    restore: vi.fn(),
  },
  spaceApi: { listMembers: vi.fn() },
}));

describe("entry revision review route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    vi.mocked(entryApi.get).mockResolvedValue({
      id: "entry-1",
      form: "Task",
      fields: { Body: "Current" },
      revision_id: "rev-current",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
    vi.mocked(entryApi.getRevision).mockResolvedValue({
      revision_id: "rev-old",
      timestamp: "2026-01-01T00:00:00Z",
      form: "Task",
      operation: "upsert",
      entry_version: 1,
      author: "creator",
      updated_by: "creator",
      markdown: "# Historical title\n\n## Body\nOriginal",
      fields: { Body: "Original" },
    });
    vi.mocked(spaceApi.listMembers).mockResolvedValue([]);
    vi.mocked(entryApi.restore).mockResolvedValue({
      revision_id: "rev-new",
      restored_from: "rev-old",
    });
  });

  it("PR3: reviews the revision read-only and restores to the Entry route", async () => {
    const { container } = render(() => <SpaceEntryRevisionRoute />);

    // Shared locale-aware subtitle plus a localized read-only marker.
    const subtitle = await screen.findByText(/Read-only$/);
    expect(subtitle).toHaveClass("revision-subtitle");
    expect(subtitle.textContent).toContain("·");

    // Shared read-only renderer: values as text, never disabled inputs.
    // Title-less Entry: no Entry-level title control; only Form fields render.
    const fields = container.querySelector(".form.entry-field-values.readonly");
    expect(fields).not.toBeNull();
    expect(screen.queryByLabelText("Title")).toBeNull();
    expect(await screen.findByText("Original")).toBeInTheDocument();
    expect(container.querySelector("input, textarea, select")).toBeNull();

    // No two-column compare, no raw dump, no heavy metadata.
    expect(
      container.querySelector(".ui-entry-history-section"),
    ).not.toBeInTheDocument();
    expect(
      container.querySelector(".ui-entry-history-divider"),
    ).not.toBeInTheDocument();
    expect(container.querySelector("pre.code")).not.toBeInTheDocument();
    expect(container.querySelector(".ui-card")).not.toBeInTheDocument();
    expect(screen.queryByText("Current value")).not.toBeInTheDocument();
    expect(screen.queryByText("Selected historical revision"))
      .not.toBeInTheDocument();
    expect(screen.queryByText("Current title")).not.toBeInTheDocument();
    // Raw revision ids stay out of primary content; the advanced technical
    // disclosure owns them.
    const main = container.querySelector(".settingsMain")!;
    const details = main.querySelector("details")!;
    expect(main.textContent).toContain("rev-old");
    expect(details.textContent).toContain("rev-old");
    expect(
      main.textContent?.replace(details.textContent ?? "", ""),
    ).not.toContain("rev-old");

    // Primary action is restore only (plus copy helpers in details).
    const restore = await screen.findByRole("button", {
      name: "Restore this revision",
    });
    expect(restore).toHaveTextContent("Restore");

    fireEvent.click(restore);

    // PR4: restore runs behind a confirmation dialog stating append-only
    // semantics; nothing is sent until Confirm restore is activated.
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(
      screen.getByText(
        /Restore creates a new revision from this version/,
      ),
    ).toBeInTheDocument();
    expect(entryApi.restore).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", { name: "Confirm restore" }),
    );

    expect(entryApi.restore).toHaveBeenCalledWith(
      "default",
      "entry-1",
      "rev-old",
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/spaces/default/entries/entry-1")
    );
  });

  it("REQ-UX-ENTRY-HISTORY-001: keeps exact revision and actor IDs in collapsed details and copies them", async () => {
    const revisionId = "revision-id-opaque";
    const actorId = "principal-id-unresolved";
    vi.mocked(entryApi.getRevision).mockResolvedValue({
      revision_id: revisionId,
      timestamp: "2026-01-01T00:00:00Z",
      form: "Task",
      operation: "upsert",
      entry_version: 1,
      actor: actorId,
      markdown: "# Historical title\n\n## Body\nOriginal",
      fields: { Body: "Original" },
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    const previousClipboard = Object.getOwnPropertyDescriptor(
      navigator,
      "clipboard",
    );
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    try {
      const { container } = render(() => <SpaceEntryRevisionRoute />);

      expect(await screen.findByText(/Unknown actor/)).toBeInTheDocument();
      const main = container.querySelector(".settingsMain")!;
      const details = main.querySelector("details.revision-technical-details")!;
      expect(details.open).toBe(false);
      const primaryContent = main.textContent?.replace(
        details.textContent ?? "",
        "",
      ) ?? "";
      expect(primaryContent).not.toContain(revisionId);
      expect(primaryContent).not.toContain(actorId);
      expect(primaryContent).not.toContain("principal-id");
      for (
        const control of main.querySelectorAll<HTMLElement>(
          "[aria-label], [title]",
        )
      ) {
        if (details.contains(control)) continue;
        expect(control.getAttribute("aria-label") ?? "").not.toContain(
          revisionId,
        );
        expect(control.getAttribute("aria-label") ?? "").not.toContain(
          actorId,
        );
        expect(control.getAttribute("title") ?? "").not.toContain(
          revisionId,
        );
        expect(control.getAttribute("title") ?? "").not.toContain(actorId);
      }
      expect(
        screen.getByRole("heading", { name: "Revision" }).textContent ?? "",
      ).not.toContain(revisionId);
      fireEvent.click(details.querySelector("summary")!);
      expect(details.open).toBe(true);
      expect(details.textContent).toContain(revisionId);
      expect(details.textContent).toContain(actorId);

      fireEvent.click(
        screen.getByRole("button", { name: `Copy ${revisionId}` }),
      );
      fireEvent.click(
        screen.getByRole("button", { name: `Copy ${actorId}` }),
      );
      await waitFor(() =>
        expect(writeText.mock.calls).toEqual([[revisionId], [actorId]])
      );
    } finally {
      if (previousClipboard) {
        Object.defineProperty(navigator, "clipboard", previousClipboard);
      } else {
        Reflect.deleteProperty(navigator, "clipboard");
      }
    }
  });

  it("PR4: cancelling the restore dialog sends nothing and keeps the review", async () => {
    render(() => <SpaceEntryRevisionRoute />);

    const restore = await screen.findByRole("button", {
      name: "Restore this revision",
    });
    fireEvent.click(restore);
    await screen.findByRole("dialog");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    expect(entryApi.restore).not.toHaveBeenCalled();
    expect(navigate).not.toHaveBeenCalled();
    // The review stays mounted with the restore action available.
    expect(
      screen.getByRole("button", { name: "Restore this revision" }),
    ).toBeInTheDocument();
  });

  it("REQ-UX-NAV-001: exposes exactly one back control to history", async () => {
    render(() => <SpaceEntryRevisionRoute />);

    const back = await screen.findByRole("link", {
      name: "Back to history",
    });
    expect(back).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1/history",
    );
    expect(back).toHaveAttribute("title", "Back to history");
    expect(screen.getAllByRole("link", { name: "Back to history" }))
      .toHaveLength(1);
    expectBackLinkAtHeaderStart(back);
  });
});
