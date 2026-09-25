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
const routeControls = vi.hoisted(() => ({
  setSpace: (_spaceId: string) => {},
  setSql: (_sqlId: string) => {},
}));

vi.mock("@solidjs/router", async () => {
  const { createSignal } = await import("solid-js");
  const [spaceId, setSpace] = createSignal("default");
  const [sqlId, setSql] = createSignal("saved-query");
  routeControls.setSpace = setSpace;
  routeControls.setSql = setSql;
  return {
    A: (props: { href: string; class?: string; children: unknown }) => (
      <a href={props.href} class={props.class}>{props.children}</a>
    ),
    useLocation: () => ({ state: undefined }),
    useNavigate: () => navigateMock,
    useParams: () => ({
      get space_id() {
        return spaceId();
      },
      get sql_id() {
        return sqlId();
      },
    }),
  };
});

vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: {
    get: getMock,
    query: queryMock,
    count: countMock,
  },
}));

describe("/spaces/:space_id/sql/:sql_id/run", () => {
  beforeEach(() => {
    routeControls.setSpace("default");
    routeControls.setSql("saved-query");
    navigateMock.mockReset();
    getMock.mockReset();
    queryMock.mockReset();
    countMock.mockReset();
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
      })
      .mockResolvedValueOnce({
        columns: ["value", "details"],
        rows: [["first", { ok: true }]],
        has_more: true,
        next: "opaque-next",
      });
    countMock.mockResolvedValue(2);
  });

  it("renders arbitrary SQL rows and keeps pagination client-held", async () => {
    render(() => <SpaceSqlRunRoute />);

    expect(await screen.findByRole("columnheader", { name: "value" }))
      .toBeInTheDocument();
    expect(screen.getByText('{"ok":true}')).toBeInTheDocument();
    expect(screen.getByText("Page 1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(queryMock).toHaveBeenCalledTimes(2));
    expect(queryMock).toHaveBeenLastCalledWith("default", {
      sql: "SELECT value FROM demo ORDER BY value",
      parameters: {},
      parameter_types: {},
      limit: 100,
      continuation: "opaque-next",
    }, expect.any(AbortSignal));
    expect(await screen.findByText("second")).toBeInTheDocument();
    expect(screen.getByText("Page 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    await waitFor(() => expect(queryMock).toHaveBeenCalledTimes(3));
    await screen.findByText("first");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(queryMock).toHaveBeenLastCalledWith("default", {
      sql: "SELECT value FROM demo ORDER BY value",
      parameters: {},
      parameter_types: {},
      limit: 100,
    }, expect.any(AbortSignal));
  });

  it("does not count while loading a page and only counts on explicit action", async () => {
    render(() => <SpaceSqlRunRoute />);
    await screen.findByRole("columnheader", { name: "value" });
    expect(countMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    await waitFor(() => expect(countMock).toHaveBeenCalledTimes(1));
    expect(countMock).toHaveBeenCalledWith("default", {
      sql: "SELECT value FROM demo ORDER BY value",
      parameters: {},
      parameter_types: {},
    }, expect.any(AbortSignal));
    expect(screen.getByText("2 rows")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

  it("offers a retry after a SQL page read fails", async () => {
    queryMock.mockReset()
      .mockRejectedValueOnce(new Error("temporary failure"))
      .mockResolvedValueOnce({
        columns: ["value"],
        rows: [["recovered"]],
        has_more: false,
      });
    render(() => <SpaceSqlRunRoute />);

    const retry = await screen.findByRole("button", { name: "Retry" });
    retry.click();
    expect(await screen.findByText("recovered")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(queryMock).toHaveBeenCalledTimes(2);
    expect(countMock).not.toHaveBeenCalled();
  });

  it("aborts the old Space page request and never renders its late response", async () => {
    let resolveOld:
      | ((page: {
        columns: string[];
        rows: unknown[][];
        has_more: boolean;
      }) => void)
      | undefined;
    queryMock.mockReset()
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveOld = resolve;
        })
      )
      .mockResolvedValueOnce({
        columns: ["space"],
        rows: [["new-space"]],
        has_more: false,
      });
    render(() => <SpaceSqlRunRoute />);
    await waitFor(() => expect(queryMock).toHaveBeenCalledTimes(1));
    const oldSignal = queryMock.mock.calls[0][2] as AbortSignal;

    routeControls.setSpace("new-space-id");

    expect(await screen.findByText("new-space")).toBeInTheDocument();
    expect(oldSignal.aborted).toBe(true);
    resolveOld?.({
      columns: ["space"],
      rows: [["old-space"]],
      has_more: false,
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByText("old-space")).not.toBeInTheDocument();
    expect(getMock).toHaveBeenCalledWith("new-space-id", "saved-query");
  });

  it("aborts an explicit count when its SQL identity changes", async () => {
    let resolveCount: ((count: number) => void) | undefined;
    countMock.mockReset().mockImplementationOnce(() =>
      new Promise((resolve) => {
        resolveCount = resolve;
      })
    );
    render(() => <SpaceSqlRunRoute />);
    await screen.findByRole("columnheader", { name: "value" });
    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    await waitFor(() => expect(countMock).toHaveBeenCalledTimes(1));
    const oldSignal = countMock.mock.calls[0][2] as AbortSignal;

    routeControls.setSql("other-sql");

    await waitFor(() =>
      expect(getMock).toHaveBeenCalledWith(
        "default",
        "other-sql",
      )
    );
    expect(oldSignal.aborted).toBe(true);
    resolveCount?.(999);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByText("999 rows")).not.toBeInTheDocument();
  });
});
