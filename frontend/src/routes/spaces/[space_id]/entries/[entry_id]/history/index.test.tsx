import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import { formatDateTimeLabel } from "~/lib/date-format";
import { entryApi } from "~/lib/ugoite-client";
import SpaceEntryHistoryRoute from "./index";

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
  useParams: () => ({ space_id: "default", entry_id: "entry-1" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  entryApi: { history: vi.fn() },
}));

describe("entry history route", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    setLocale("en");
  });

  it("renders the backend revision timestamp", async () => {
    const timestamp = 1767225600;
    vi.mocked(entryApi.history).mockResolvedValue({
      revisions: [{
        revision_id: "rev-1",
        timestamp,
        checksum: "checksum",
      }],
    });

    render(() => <SpaceEntryHistoryRoute />);

    expect(await screen.findByText(formatDateTimeLabel(timestamp)))
      .toBeInTheDocument();
  });

  it("REQ-UX-NAV-001: exposes exactly one back control to the entry", async () => {
    vi.mocked(entryApi.history).mockResolvedValue({ revisions: [] });

    render(() => <SpaceEntryHistoryRoute />);

    const back = await screen.findByRole("link", { name: "Back to Entry" });
    expect(back).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1",
    );
    expect(back).toHaveAttribute("title", "Back to Entry");
    expect(screen.getAllByRole("link", { name: "Back to Entry" }))
      .toHaveLength(1);
    // The space-history shortcut is gone: the shell owns that destination.
    expect(screen.queryByRole("link", { name: "View space history" }))
      .not.toBeInTheDocument();
  });

  it("routes revisions through the single History path with no /restore links", async () => {
    vi.mocked(entryApi.history).mockResolvedValue({
      revisions: [{
        revision_id: "rev-1",
        timestamp: 1767225600,
        checksum: "checksum",
      }],
    });

    const { container } = render(() => <SpaceEntryHistoryRoute />);

    const revisionLink = await screen.findByRole("link", {
      name: "Changed",
    });
    expect(revisionLink).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1/history/rev-1",
    );
    expect(
      container.querySelector('a[href$="/restore"]'),
    ).not.toBeInTheDocument();
  });

  it("PR3: renders a 3-column history table with chevron only", async () => {
    vi.mocked(entryApi.history).mockResolvedValue({
      revisions: [{
        revision_id: "rev-1",
        timestamp: 1767225600,
        checksum: "checksum",
        title: "Hidden title",
        form: "Hidden form",
        operation: "upsert",
        entry_version: 2,
        actor: "alice",
      }],
    });

    const { container } = render(() => <SpaceEntryHistoryRoute />);

    const table = await screen.findByRole("table");
    expect(table).toHaveClass("dataTable");
    expect(table).toHaveClass("entry-history-table");
    expect(table.closest(".tablewrap")).not.toBeNull();

    const headers = [...table.querySelectorAll("thead th")].map((th) =>
      th.textContent?.trim()
    );
    expect(headers.slice(0, 3)).toEqual([
      "Operation",
      "Actor",
      "Timestamp",
    ]);

    // One row link (the operation) routes to the revision; revision id,
    // title, form, and summary stay out of the table.
    const rowLink = await screen.findByRole("link", { name: "Updated" });
    expect(rowLink).toHaveAttribute(
      "href",
      "/spaces/default/entries/entry-1/history/rev-1",
    );
    expect(screen.getByText("alice")).toBeInTheDocument();
    expect(
      await screen.findByText(formatDateTimeLabel(1767225600)),
    ).toBeInTheDocument();
    expect(screen.queryByText(/rev-1/)).toBeNull();
    expect(screen.queryByText("Hidden title")).toBeNull();
    expect(screen.queryByText("Hidden form")).toBeNull();
    expect(container.querySelector("tbody .chev")).toHaveTextContent("›");
  });

  it("PR3: pagination keeps rows with a footer spinner while loading more", async () => {
    const revisions = Array.from({ length: 51 }, (_, index) => ({
      revision_id: `rev-${index}`,
      timestamp: 1767225600 + index,
      checksum: `checksum-${index}`,
      operation: "upsert",
      entry_version: 2,
      actor: "alice",
    }));
    vi.mocked(entryApi.history).mockResolvedValueOnce({ revisions });
    let resolveMore!: (value: { revisions: typeof revisions }) => void;
    vi.mocked(entryApi.history).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveMore = resolve;
        }),
    );

    const { container } = render(() => <SpaceEntryHistoryRoute />);

    await screen.findByRole("table");
    expect(container.querySelectorAll("tbody tr")).toHaveLength(50);

    const loadMore = await screen.findByRole("button", {
      name: "Load more history",
    });
    fireEvent.click(loadMore);

    // Rows stay mounted; only a small footer spinner appears next to the
    // button (spinner-only status, no visible loading text).
    await waitFor(() => {
      expect(
        container.querySelector('.localpending-sm[role="status"]'),
      ).not.toBeNull();
    });
    expect(container.querySelectorAll("tbody tr")).toHaveLength(50);
    const footerStatus = container.querySelector(
      '.localpending-sm[role="status"]',
    )!;
    expect(footerStatus.querySelector(".localspinner")).not.toBeNull();
    expect(footerStatus.querySelector(".ui-sr-only")).not.toBeNull();

    resolveMore({ revisions: [] });
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Load more history" }),
      ).not.toBeInTheDocument();
    });
    expect(container.querySelectorAll("tbody tr")).toHaveLength(50);
  });
});
