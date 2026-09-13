import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryRevisionRoute from "./[revision_id]";

const navigate = vi.fn();

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
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

  it("reviews current versus historical values and restores to the Entry route", async () => {
    render(() => <SpaceEntryRevisionRoute />);

    expect(await screen.findByText("Current value")).toBeInTheDocument();
    expect(await screen.findByText("Selected historical revision"))
      .toBeInTheDocument();
    expect(await screen.findByText("Current title")).toBeInTheDocument();
    expect(await screen.findByText("Historical title")).toBeInTheDocument();
    expect(await screen.findByText(/Original$/)).toBeInTheDocument();
    expect(await screen.findByText(/Restore appends a new current revision/))
      .toBeInTheDocument();

    fireEvent.click(
      await screen.findByRole("button", { name: "Restore this revision" }),
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
});
