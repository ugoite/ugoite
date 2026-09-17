import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryRevisionRoute from "./[revision_id]";

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
}));

describe("entry revision review route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
    vi.mocked(entryApi.get).mockResolvedValue({
      id: "entry-1",
      title: "Current title",
      form: "Task",
      content: "# Current title\n\n## Body\nCurrent",
      revision_id: "rev-current",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-02T00:00:00Z",
    });
    vi.mocked(entryApi.getRevision).mockResolvedValue({
      revision_id: "rev-old",
      timestamp: "2026-01-01T00:00:00Z",
      title: "Historical title",
      form: "Task",
      operation: "upsert",
      entry_version: 1,
      author: "creator",
      updated_by: "creator",
      markdown: "# Historical title\n\n## Body\nOriginal",
    });
    vi.mocked(entryApi.restore).mockResolvedValue({
      id: "entry-1",
      revision_id: "rev-new",
      content: "",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-03T00:00:00Z",
    });
  });

  it("PR3: reviews the revision read-only and restores to the Entry route", async () => {
    const { container } = render(() => <SpaceEntryRevisionRoute />);

    // Shared locale-aware subtitle plus a localized read-only marker.
    const subtitle = await screen.findByText(/Read-only$/);
    expect(subtitle).toHaveClass("revision-subtitle");
    expect(subtitle.textContent).toContain("·");

    // Shared read-only fields: every control disabled, wrapper marked.
    const fields = container.querySelector(".form.entry-fields.readonly");
    expect(fields).not.toBeNull();
    expect(await screen.findByLabelText("Title")).toBeDisabled();
    expect(await screen.findByLabelText("Body")).toBeDisabled();
    expect(screen.getByLabelText("Title")).toHaveValue("Historical title");
    expect(screen.getByLabelText("Body")).toHaveValue("Original");

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
    expect(screen.queryByText(/rev-old/)).toBeNull();

    // Destructive-restore warning stays visible as an alert.
    expect(await screen.findByText(/Restore appends a new current revision/))
      .toBeInTheDocument();

    // Primary action is restore only.
    const buttons = container.querySelectorAll('button[type="button"]');
    expect(buttons).toHaveLength(1);
    const restore = await screen.findByRole("button", {
      name: "Restore this revision",
    });
    expect(restore).toHaveTextContent("Restore");

    fireEvent.click(restore);

    expect(entryApi.restore).toHaveBeenCalledWith(
      "default",
      "entry-1",
      "rev-old",
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/spaces/default/entries/entry-1")
    );
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
  });
});
