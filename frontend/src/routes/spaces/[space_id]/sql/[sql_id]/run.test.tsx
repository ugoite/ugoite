import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceSqlRunRoute from "./run";

const { navigateMock, getMock, queryMock, countMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  getMock: vi.fn(),
  queryMock: vi.fn(),
  countMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children}</a>
  ),
  useLocation: () => ({ state: undefined }),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default", sql_id: "saved-query" }),
}));

vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: {
    get: getMock,
    query: queryMock,
    count: countMock,
  },
}));

describe("/spaces/:space_id/sql/:sql_id/run", () => {
  beforeEach(() => {
    navigateMock.mockReset();
    getMock.mockResolvedValue({
      id: "saved-query",
      name: "Saved query",
      kind: "user-query",
      sql: "SELECT value FROM demo ORDER BY value",
      variables: [],
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-02T00:00:00Z",
      revision_id: "rev-1",
    });
    queryMock
      .mockResolvedValueOnce({
        columns: ["value", "details"],
        rows: [["first", { ok: true }]],
        has_more: true,
        next: "opaque-next",
      })
      .mockResolvedValueOnce({
        columns: ["value", "details"],
        rows: [{ value: "second", details: null }],
        has_more: false,
      });
    countMock.mockResolvedValue(2);
  });

  it("renders arbitrary SQL rows and keeps pagination client-held", async () => {
    render(() => <SpaceSqlRunRoute />);

    expect(await screen.findByRole("columnheader", { name: "value" }))
      .toBeInTheDocument();
    expect(screen.getByText('{"ok":true}')).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(queryMock).toHaveBeenCalledTimes(2));
    expect(queryMock).toHaveBeenLastCalledWith("default", {
      sql: "SELECT value FROM demo ORDER BY value",
      parameters: {},
      parameter_types: {},
      limit: 100,
      continuation: "opaque-next",
    });
    expect(await screen.findByText("second")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    await waitFor(() => expect(queryMock).toHaveBeenCalledTimes(3));
    expect(queryMock).toHaveBeenLastCalledWith("default", {
      sql: "SELECT value FROM demo ORDER BY value",
      parameters: {},
      parameter_types: {},
      limit: 100,
    });
  });

  it("does not count while loading a page and only counts on explicit action", async () => {
    render(() => <SpaceSqlRunRoute />);
    await screen.findByRole("columnheader", { name: "value" });
    expect(countMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    await waitFor(() => expect(countMock).toHaveBeenCalledTimes(1));
    expect(screen.getByText("2 rows")).toBeInTheDocument();
  });
});
