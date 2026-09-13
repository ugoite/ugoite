import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { formatDateTimeLabel } from "~/lib/date-format";
import { setLocale } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryRestoreRoute from "./restore";

const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useNavigate: () => navigate,
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: {
    history: vi.fn(),
    get: vi.fn(),
    getRevision: vi.fn(),
    restore: vi.fn(),
  },
}));

describe("entry restore route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
  });

  it("renders a string revision timestamp", async () => {
    const timestamp = "2026-01-01T00:00:00Z";
    vi.mocked(entryApi.history).mockResolvedValue({
      revisions: [{
        revision_id: "rev-1",
        timestamp,
        checksum: "checksum",
      }],
    });

    render(() => <SpaceEntryRestoreRoute />);

    expect(await screen.findByText(formatDateTimeLabel(timestamp)))
      .toBeInTheDocument();
  });

  it("reviews the selected revision before appending restore and reopening the Entry", async () => {
    vi.mocked(entryApi.history).mockResolvedValue({
      revisions: [{
        revision_id: "rev-1",
        timestamp: "2026-01-01T00:00:00Z",
        checksum: "checksum",
        operation: "upsert",
        entry_version: 1,
        actor: "creator",
        title: "Original title",
        form: "Task",
      }],
    });
    vi.mocked(entryApi.get).mockResolvedValue({
      id: "entry-1",
      title: "Current title",
      form: "Task",
      content: "# Current title",
      revision_id: "rev-2",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-02T00:00:00Z",
    });
    vi.mocked(entryApi.getRevision).mockResolvedValue({
      revision_id: "rev-1",
      timestamp: "2026-01-01T00:00:00Z",
      title: "Original title",
      form: "Task",
      markdown: "# Original title",
    });
    vi.mocked(entryApi.restore).mockResolvedValue({
      id: "entry-1",
      revision_id: "rev-3",
      content: "",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-03T00:00:00Z",
    });

    render(() => <SpaceEntryRestoreRoute />);

    fireEvent.click(
      await screen.findByRole("radio", { name: /Created: Original title/ }),
    );
    expect(await screen.findByText("Current title")).toBeInTheDocument();
    expect(await screen.findAllByText("Original title")).toHaveLength(2);
    fireEvent.click(
      await screen.findByRole("button", { name: "Restore this revision" }),
    );

    expect(entryApi.restore).toHaveBeenCalledWith(
      "default",
      "entry-1",
      "rev-1",
    );
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith("/spaces/default/entries/entry-1")
    );
  });
});
