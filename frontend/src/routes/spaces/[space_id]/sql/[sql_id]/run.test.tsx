import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { compositionApi } from "~/lib/composition-api";
import {
  clearPendingCompositionSaveAttempt,
  getPendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import SpaceSqlRunRoute from "./run";

const {
  navigateMock,
  getMock,
  queryMock,
  countMock,
} = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  getMock: vi.fn(),
  queryMock: vi.fn(),
  countMock: vi.fn(),
}));
const routeControls = vi.hoisted(() => ({
  setSpace: (_spaceId: string) => {},
  setSql: (_sqlId: string) => {},
  setPath: (_path: string) => {},
  setState: (_state: unknown) => {},
}));

vi.mock("@solidjs/router", async () => {
  const { createSignal } = await import("solid-js");
  const [spaceId, setSpace] = createSignal("default");
  const [sqlId, setSql] = createSignal("saved-query");
  const [path, setPath] = createSignal(
    "/spaces/default/sql/saved-query/run",
  );
  const [state, setState] = createSignal<unknown>(undefined);
  routeControls.setSpace = setSpace;
  routeControls.setSql = setSql;
  routeControls.setPath = setPath;
  routeControls.setState = setState;
  return {
    A: (props: { href: string; class?: string; children: unknown }) => (
      <a href={props.href} class={props.class}>{props.children}</a>
    ),
    useLocation: () => ({
      get pathname() {
        return path();
      },
      get state() {
        return state();
      },
    }),
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
  canonicalizeCompositionDocument: vi.fn(),
  evaluateCompositionMetricPage: vi.fn(),
  protocolFetch: vi.fn(),
}));

const canonicalizeMock = vi.spyOn(compositionApi, "canonicalizeDocument");
const compositionSaveMock = vi.spyOn(compositionApi, "save");

describe("/spaces/:space_id/sql/:sql_id/run", () => {
  beforeEach(() => {
    routeControls.setSpace("default");
    routeControls.setSql("saved-query");
    routeControls.setPath("/spaces/default/sql/saved-query/run");
    clearPendingCompositionSaveAttempt({
      spaceId: "default",
      sqlId: "saved-query",
      routePath: "/spaces/default/sql/saved-query/run",
    });
    routeControls.setState(undefined);
    navigateMock.mockReset();
    getMock.mockReset();
    queryMock.mockReset();
    countMock.mockReset();
    canonicalizeMock.mockReset().mockResolvedValue({
      document: {},
      canonical_yaml: "canonical composition yaml",
      fingerprint: "fingerprint",
    });
    compositionSaveMock.mockReset().mockResolvedValue({
      composition_id: "tool-1",
      revision_id: "tool-revision-3",
      canonical_yaml: "canonical composition yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: "snapshot-1",
        committed_revision_ids: ["tool-revision-3"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    });
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
      saved_sql: { id: "saved-query", revision_id: "rev-1" },
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
      saved_sql: { id: "saved-query", revision_id: "rev-1" },
    }, expect.any(AbortSignal));
  });

  it("keeps duplicate SQL column names attached to their original positions", async () => {
    queryMock.mockReset().mockResolvedValueOnce({
      columns: ["same", "same"],
      rows: [["left", "right"]],
      has_more: false,
    });
    render(() => <SpaceSqlRunRoute />);

    expect(await screen.findAllByRole("columnheader", { name: "same" }))
      .toHaveLength(2);
    expect(screen.getByText("left")).toBeInTheDocument();
    expect(screen.getByText("right")).toBeInTheDocument();
    expect(countMock).not.toHaveBeenCalled();
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
      saved_sql: { id: "saved-query", revision_id: "rev-1" },
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

  it("saves as a tool and retries an uncertain write with identical YAML and key", async () => {
    const savedSql = {
      id: "saved-query",
      name: "Monthly query",
      kind: "user-query" as const,
      sql: "SELECT amount FROM expenses WHERE month >= :month_start",
      variables: [{ name: "month_start", type: "date", description: "" }],
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-02T00:00:00Z",
      revision_id: "sql-revision-8",
    };
    getMock.mockResolvedValue(savedSql);
    routeControls.setState({
      parameters: { month_start: "2026-01-01" },
      parameterTypes: { month_start: "date" },
    });
    compositionSaveMock
      .mockRejectedValueOnce(new Error("response lost"))
      .mockResolvedValueOnce({
        composition_id: "tool-1",
        revision_id: "tool-revision-3",
        canonical_yaml: "canonical composition yaml",
        receipt: {
          command_id: "command-1",
          catalog_generation: 4,
          snapshot_id: "snapshot-1",
          committed_revision_ids: ["tool-revision-3"],
          committed_at_micros: 1,
          data_file_count: 1,
        },
      });
    render(() => <SpaceSqlRunRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Save as tool" });
    expect(screen.getByLabelText("Name", { selector: "input" }))
      .toHaveValue("Monthly query");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByRole("alert"))
      .toHaveTextContent("Could not save this tool.");

    const firstCall = compositionSaveMock.mock.calls[0];
    expect(firstCall[0]).toBe("default");
    expect(firstCall[1]).toBe("canonical composition yaml");
    expect(firstCall[2]).toEqual(expect.any(String));
    expect(canonicalizeMock).toHaveBeenCalledWith(expect.objectContaining({
      name: "Monthly query",
      spec: expect.objectContaining({
        sources: [expect.objectContaining({
          entry_id: "saved-query",
          revision_id: "sql-revision-8",
          variables: { month_start: { parameter: "month_start" } },
        })],
      }),
    }));

    fireEvent.click(within(dialog).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(2));
    expect(compositionSaveMock.mock.calls[1]).toEqual(firstCall);
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/compositions/tool-1/tool-revision-3",
      )
    );
  });

  it("only offers save-as after a successful user-query result", async () => {
    getMock.mockResolvedValueOnce({
      id: "saved-query",
      name: "History query",
      kind: "search-history",
      sql: "SELECT 1",
      variables: [],
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-02T00:00:00Z",
      revision_id: "sql-revision-1",
    });
    render(() => <SpaceSqlRunRoute />);
    await screen.findByRole("columnheader", { name: "value" });
    expect(screen.queryByRole("button", { name: "Save as tool" }))
      .not.toBeInTheDocument();

    cleanup();
    getMock.mockReset().mockRejectedValue(new Error("unavailable"));
    render(() => <SpaceSqlRunRoute />);
    await screen.findByRole("alert");
    expect(screen.queryByRole("button", { name: "Save as tool" }))
      .not.toBeInTheDocument();
  });

  it("does not navigate after a save resolves off the run route", async () => {
    let resolveSave:
      | ((value: Awaited<ReturnType<typeof compositionApi.save>>) => void)
      | undefined;
    compositionSaveMock.mockImplementationOnce(() =>
      new Promise((resolve) => {
        resolveSave = resolve;
      })
    );
    render(() => <SpaceSqlRunRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "Save as tool" }))
        .getByRole("button", { name: "Save" }),
    );
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(1));

    routeControls.setPath("/spaces/default/sql/saved-query");
    resolveSave?.({
      composition_id: "tool-1",
      revision_id: "tool-revision-3",
      canonical_yaml: "canonical composition yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: "snapshot-1",
        committed_revision_ids: ["tool-revision-3"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    });

    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Save as tool" }),
      ).not.toBeInTheDocument()
    );
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("retries a committed save with the same key after leaving and returning", async () => {
    const committedPublications = new Map<string, {
      composition_id: string;
      revision_id: string;
      canonical_yaml: string;
      receipt: {
        command_id: string;
        catalog_generation: number;
        snapshot_id: string;
        committed_revision_ids: string[];
        committed_at_micros: number;
        data_file_count: number;
      };
    }>();
    let loseFirstResponse: ((error: unknown) => void) | undefined;
    compositionSaveMock.mockImplementation((spaceId, yaml, idempotencyKey) => {
      const existing = committedPublications.get(idempotencyKey);
      if (existing) return Promise.resolve(existing);
      const publication = {
        composition_id: "tool-1",
        revision_id: `tool-revision-${committedPublications.size + 1}`,
        canonical_yaml: yaml,
        receipt: {
          command_id: `command-${committedPublications.size + 1}`,
          catalog_generation: 4,
          snapshot_id: "snapshot-1",
          committed_revision_ids: [
            `tool-revision-${committedPublications.size + 1}`,
          ],
          committed_at_micros: 1,
          data_file_count: 1,
        },
      };
      committedPublications.set(idempotencyKey, publication);
      expect(spaceId).toBe("default");
      return new Promise((_resolve, reject) => {
        loseFirstResponse = reject;
      });
    });
    render(() => <SpaceSqlRunRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Save as tool" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(1));
    const firstCall = compositionSaveMock.mock.calls[0];
    expect(firstCall[1]).toBe("canonical composition yaml");
    expect(firstCall[2]).toEqual(expect.any(String));
    expect(committedPublications.size).toBe(1);

    routeControls.setPath("/spaces/default/sql/saved-query");
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Save as tool" }),
      ).not.toBeInTheDocument()
    );
    loseFirstResponse?.(
      Object.assign(new Error("response lost after commit"), {
        mutationOutcome: "unknown",
      }),
    );
    await waitFor(() =>
      expect(
        getPendingCompositionSaveAttempt({
          spaceId: "default",
          sqlId: "saved-query",
          routePath: "/spaces/default/sql/saved-query/run",
        }),
      ).toEqual(expect.objectContaining({
        yaml: firstCall[1],
        idempotencyKey: firstCall[2],
      }))
    );
    expect(navigateMock).not.toHaveBeenCalled();

    cleanup();
    routeControls.setPath("/spaces/default/sql/saved-query/run");
    queryMock.mockReset().mockResolvedValue({
      columns: ["value"],
      rows: [["result"]],
      has_more: false,
    });
    render(() => <SpaceSqlRunRoute />);
    const recoveredDialog = await screen.findByRole("dialog", {
      name: "Save as tool",
    });
    expect(screen.getByLabelText("Name", { selector: "input" }))
      .toHaveValue("Saved query");
    fireEvent.click(
      within(recoveredDialog).getByRole("button", { name: "Retry" }),
    );

    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(2));
    expect(compositionSaveMock.mock.calls[1]).toEqual(firstCall);
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/compositions/tool-1/tool-revision-1",
      )
    );
    expect(committedPublications.size).toBe(1);
    expect(
      [...committedPublications.values()].map(({ revision_id }) => revision_id),
    )
      .toEqual(["tool-revision-1"]);
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

  it.each(["values", "types"] as const)(
    "aborts page and count reads when SQL parameter %s change",
    async (changedField) => {
      let resolveNextPage:
        | ((page: {
          columns: string[];
          rows: unknown[][];
          has_more: boolean;
        }) => void)
        | undefined;
      let resolveCount: ((count: number) => void) | undefined;
      queryMock.mockReset()
        .mockResolvedValueOnce({
          columns: ["value"],
          rows: [["initial"]],
          has_more: true,
          next: "next-page",
        })
        .mockImplementationOnce(() =>
          new Promise((resolve) => {
            resolveNextPage = resolve;
          })
        )
        .mockResolvedValueOnce({
          columns: ["value"],
          rows: [["updated"]],
          has_more: false,
        });
      countMock.mockReset().mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveCount = resolve;
        })
      );
      routeControls.setState({
        parameters: { threshold: 10 },
        parameterTypes: { threshold: "integer" },
      });
      render(() => <SpaceSqlRunRoute />);

      expect(await screen.findByText("initial")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Next" }));
      await waitFor(() => expect(queryMock).toHaveBeenCalledTimes(2));
      fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
      await waitFor(() => expect(countMock).toHaveBeenCalledTimes(1));
      const oldPageSignal = queryMock.mock.calls[1][2] as AbortSignal;
      const oldCountSignal = countMock.mock.calls[0][2] as AbortSignal;

      routeControls.setState({
        parameters: { threshold: changedField === "values" ? 11 : 10 },
        parameterTypes: {
          threshold: changedField === "types" ? "string" : "integer",
        },
      });

      expect(await screen.findByText("updated")).toBeInTheDocument();
      expect(oldPageSignal.aborted).toBe(true);
      expect(oldCountSignal.aborted).toBe(true);
      resolveNextPage?.({
        columns: ["value"],
        rows: [["stale-page"]],
        has_more: false,
      });
      resolveCount?.(999);
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(screen.queryByText("stale-page")).not.toBeInTheDocument();
      expect(screen.queryByText("999 rows")).not.toBeInTheDocument();
      expect(queryMock).toHaveBeenLastCalledWith("default", {
        sql: "SELECT value FROM demo ORDER BY value",
        parameters: { threshold: changedField === "values" ? 11 : 10 },
        parameter_types: {
          threshold: changedField === "types" ? "string" : "integer",
        },
        limit: 100,
        saved_sql: { id: "saved-query", revision_id: "rev-1" },
      }, expect.any(AbortSignal));
    },
  );

  it("aborts outstanding page and count reads when the route is disposed", async () => {
    let resolveNextPage:
      | ((page: {
        columns: string[];
        rows: unknown[][];
        has_more: boolean;
      }) => void)
      | undefined;
    let resolveCount: ((count: number) => void) | undefined;
    queryMock.mockReset()
      .mockResolvedValueOnce({
        columns: ["value"],
        rows: [["initial"]],
        has_more: true,
        next: "next-page",
      })
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveNextPage = resolve;
        })
      );
    countMock.mockReset().mockImplementationOnce(() =>
      new Promise((resolve) => {
        resolveCount = resolve;
      })
    );
    const view = render(() => <SpaceSqlRunRoute />);

    expect(await screen.findByText("initial")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(queryMock).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    await waitFor(() => expect(countMock).toHaveBeenCalledTimes(1));
    const pageSignal = queryMock.mock.calls[1][2] as AbortSignal;
    const countSignal = countMock.mock.calls[0][2] as AbortSignal;

    view.unmount();

    expect(pageSignal.aborted).toBe(true);
    expect(countSignal.aborted).toBe(true);
    resolveNextPage?.({
      columns: ["value"],
      rows: [["late-page"]],
      has_more: false,
    });
    resolveCount?.(999);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(view.container).toBeEmptyDOMElement();
  });

  it("retries an explicit count independently after a count failure", async () => {
    countMock.mockReset()
      .mockRejectedValueOnce(new Error("temporary count failure"))
      .mockResolvedValueOnce(3);
    render(() => <SpaceSqlRunRoute />);
    expect(await screen.findByText("first")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    expect(
      await screen.findByText(
        /Failed to count query rows.*temporary count failure/,
      ),
    )
      .toBeInTheDocument();
    expect(queryMock).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Count rows" }));
    expect(await screen.findByText("3 rows")).toBeInTheDocument();
    expect(queryMock).toHaveBeenCalledTimes(1);
    expect(countMock).toHaveBeenCalledTimes(2);
  });
});
