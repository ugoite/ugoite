import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import CompositionHistoryRoute from "./history";

const { historyMock, listMock } = vi.hoisted(() => ({
  historyMock: vi.fn(),
  listMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  useParams: () => ({ space_id: "space-1", composition_id: "tool-1" }),
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    ["aria-label"]?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children as never}
    </a>
  ),
}));

vi.mock("~/lib/composition-api", () => ({
  compositionApi: {
    history: (...args: unknown[]) =>
      (historyMock as (...call: unknown[]) => unknown)(...args),
    list: (...args: unknown[]) =>
      (listMock as (...call: unknown[]) => unknown)(...args),
  },
  compositionDisplayName: (value: unknown) =>
    typeof value === "string" && value.trim() ? value.trim() : "Composition",
}));

const revision = (
  revisionId: string,
  operation: string,
  committedAtMicros: number,
) => ({
  revision: {
    entry_id: "tool-1",
    revision_id: revisionId,
    committed_at_micros: committedAtMicros,
    operation,
  },
  fields: { name: "Monthly expenses" },
  unmapped_field_values: {},
});

const firstPage = {
  entry_id: "tool-1",
  revisions: [
    revision("revision-1", "upsert", 1767225600000000),
    revision("revision-2", "restore", 1767312000000000),
  ],
  total: 2,
  offset: 0,
  limit: 50,
  has_more: false,
};

describe("Composition history route", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    historyMock.mockResolvedValue(firstPage);
    listMock.mockResolvedValue({
      items: [{
        composition_id: "tool-1",
        revision_id: "revision-2",
        updated_at: 1767312000,
        name: "Monthly expenses",
        tags: [],
      }],
      offset: 0,
      limit: 100,
      has_more: false,
    });
  });

  afterEach(() => cleanup());

  it("lists revisions with human labels and opens the exact revision", async () => {
    render(() => <CompositionHistoryRoute />);

    expect(
      await screen.findByRole("heading", { name: "Monthly expenses" }),
    ).toBeInTheDocument();
    expect(historyMock).toHaveBeenCalledWith(
      "space-1",
      "tool-1",
      50,
      0,
      expect.any(AbortSignal),
    );

    // Human operation labels with a timestamp, never a bare identifier row.
    expect(screen.getByRole("link", { name: /Created · / })).toHaveAttribute(
      "href",
      "/spaces/space-1/compositions/tool-1/revision-1",
    );
    const restored = screen.getByRole("link", { name: /Restored · / });
    expect(restored).toHaveAttribute(
      "href",
      "/spaces/space-1/compositions/tool-1/revision-2",
    );
    expect(restored).toHaveTextContent("2026");
    // The current revision is marked without showing its identifier.
    expect(restored).toHaveTextContent("Current");
    // Raw identifiers live only in the advanced disclosure.
    expect(screen.getByText("Technical details")).toBeInTheDocument();
    expect(screen.getByText("revision-1")).toBeInTheDocument();
    // One Back to the saved-tools list.
    expect(
      screen.getByRole("link", { name: "Back to saved tools" }),
    ).toHaveAttribute("href", "/spaces/space-1/compositions");
  });

  it("marks the current revision and pages through history", async () => {
    historyMock
      .mockResolvedValueOnce({ ...firstPage, has_more: true })
      .mockResolvedValueOnce({
        entry_id: "tool-1",
        revisions: [revision("revision-3", "upsert", 1767398400000000)],
        total: 3,
        offset: 50,
        limit: 50,
        has_more: false,
      });
    render(() => <CompositionHistoryRoute />);

    expect(
      await screen.findByRole("heading", { name: "Monthly expenses" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(await screen.findByRole("link", { name: /Edited · / }))
      .toHaveAttribute(
        "href",
        "/spaces/space-1/compositions/tool-1/revision-3",
      );
    expect(historyMock).toHaveBeenLastCalledWith(
      "space-1",
      "tool-1",
      50,
      50,
      expect.any(AbortSignal),
    );
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(historyMock).toHaveBeenLastCalledWith(
      "space-1",
      "tool-1",
      50,
      0,
      expect.any(AbortSignal),
    );
  });

  it("shows a retry control when history fails to load", async () => {
    historyMock
      .mockRejectedValueOnce(new Error("temporary read failure"))
      .mockResolvedValueOnce(firstPage);
    render(() => <CompositionHistoryRoute />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load tool history.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("heading", { name: "Monthly expenses" }),
    ).toBeInTheDocument();
    expect(historyMock).toHaveBeenCalledTimes(2);
  });
});
