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
        snapshot_id: 42,
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
          snapshot_id: 42,
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

  it.each(["space", "sql", "path"] as const)(
    "closes and resets the save dialog when its %s identity changes during canonicalization",
    async (changedIdentity) => {
      let resolveCanonical:
        | ((
          value: Awaited<
            ReturnType<typeof compositionApi.canonicalizeDocument>
          >,
        ) => void)
        | undefined;
      canonicalizeMock.mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveCanonical = resolve;
        })
      );
      render(() => <SpaceSqlRunRoute />);

      fireEvent.click(
        await screen.findByRole("button", { name: "Save as tool" }),
      );
      const dialog = screen.getByRole("dialog", { name: "Save as tool" });
      fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
      await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledTimes(1));

      switch (changedIdentity) {
        case "space":
          routeControls.setSpace("other-space");
          routeControls.setPath(
            "/spaces/other-space/sql/saved-query/run",
          );
          break;
        case "sql":
          routeControls.setSql("other-query");
          routeControls.setPath(
            "/spaces/default/sql/other-query/run",
          );
          break;
        case "path":
          routeControls.setPath("/spaces/default/sql/saved-query");
          break;
      }

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Save as tool" }))
          .not.toBeInTheDocument()
      );
      resolveCanonical?.({
        document: {},
        canonical_yaml: "canonical composition yaml",
        fingerprint: "fingerprint",
      });
      await new Promise((resolve) => setTimeout(resolve, 0));

      expect(compositionSaveMock).not.toHaveBeenCalled();
      expect(screen.queryByRole("dialog", { name: "Save as tool" }))
        .not.toBeInTheDocument();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    },
  );

  it("ignores a late canonicalization rejection after a new SQL save dialog opens", async () => {
    let rejectCanonical: ((error: unknown) => void) | undefined;
    canonicalizeMock.mockImplementationOnce(() =>
      new Promise((_resolve, reject) => {
        rejectCanonical = reject;
      })
    );
    getMock.mockImplementation((_spaceId, requestedSqlId) =>
      Promise.resolve({
        id: requestedSqlId,
        name: requestedSqlId === "other-query" ? "Other query" : "Saved query",
        kind: "user-query",
        sql: "SELECT value FROM demo ORDER BY value",
        variables: [],
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-02T00:00:00Z",
        revision_id: "rev-1",
      })
    );
    queryMock.mockReset().mockResolvedValue({
      columns: ["value"],
      rows: [["result"]],
      has_more: false,
    });
    render(() => <SpaceSqlRunRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "Save as tool" }))
        .getByRole("button", { name: "Save" }),
    );
    await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledTimes(1));

    routeControls.setSql("other-query");
    routeControls.setPath("/spaces/default/sql/other-query/run");
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Save as tool" }))
        .not.toBeInTheDocument()
    );
    await waitFor(() =>
      expect(getMock).toHaveBeenLastCalledWith("default", "other-query")
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    const currentDialog = screen.getByRole("dialog", {
      name: "Save as tool",
    });
    expect(screen.getByLabelText("Name", { selector: "input" }))
      .toHaveValue("Other query");

    rejectCanonical?.(new Error("canonicalization failed"));
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.getByRole("dialog", { name: "Save as tool" }))
      .toBe(currentDialog);
    expect(screen.getByLabelText("Name", { selector: "input" }))
      .toHaveValue("Other query");
    expect(within(currentDialog).getByRole("button", { name: "Save" }))
      .toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(compositionSaveMock).not.toHaveBeenCalled();
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
        snapshot_id: 42,
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
        snapshot_id: number;
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
          snapshot_id: 42,
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
        attempt: expect.objectContaining({
          yaml: firstCall[1],
          idempotencyKey: firstCall[2],
        }),
        state: "uncertain",
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

  it.each(["success", "unknown failure", "rejected"] as const)(
    "rehydrates an in-flight save after route return and retries its key after %s",
    async (outcome) => {
      const committedPublications = new Map<string, {
        composition_id: string;
        revision_id: string;
        canonical_yaml: string;
        receipt: {
          command_id: string;
          catalog_generation: number;
          snapshot_id: number;
          committed_revision_ids: string[];
          committed_at_micros: number;
          data_file_count: number;
        };
      }>();
      let firstPublication: Awaited<ReturnType<typeof compositionApi.save>>;
      let resolveFirst: (
        value: Awaited<ReturnType<typeof compositionApi.save>>,
      ) => void;
      let rejectFirst: (error: unknown) => void;
      let requestCount = 0;
      compositionSaveMock.mockImplementation(
        (_spaceId, yaml, idempotencyKey) => {
          requestCount += 1;
          const existing = committedPublications.get(idempotencyKey);
          if (existing) return Promise.resolve(existing);
          firstPublication = {
            composition_id: "tool-1",
            revision_id: `tool-revision-${committedPublications.size + 1}`,
            canonical_yaml: yaml,
            receipt: {
              command_id: "command-1",
              catalog_generation: 4,
              snapshot_id: 42,
              committed_revision_ids: ["tool-revision-1"],
              committed_at_micros: 1,
              data_file_count: 1,
            },
          };
          if (outcome !== "rejected" || requestCount > 1) {
            committedPublications.set(idempotencyKey, firstPublication);
          }
          if (requestCount > 1) return Promise.resolve(firstPublication);
          return new Promise((resolve, reject) => {
            resolveFirst = resolve;
            rejectFirst = reject;
          });
        },
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
      const firstCall = compositionSaveMock.mock.calls[0];
      const routeIdentity = {
        spaceId: "default",
        sqlId: "saved-query",
        routePath: "/spaces/default/sql/saved-query/run",
      };
      expect(getPendingCompositionSaveAttempt(routeIdentity)).toEqual(
        expect.objectContaining({
          state: "in_flight",
          attempt: expect.objectContaining({
            yaml: firstCall[1],
            idempotencyKey: firstCall[2],
          }),
        }),
      );

      routeControls.setPath("/spaces/default/sql/saved-query");
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Save as tool" }))
          .not.toBeInTheDocument()
      );
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
      expect(within(recoveredDialog).getByRole("button", { name: "Retry" }))
        .toBeEnabled();
      expect(navigateMock).not.toHaveBeenCalled();

      if (outcome === "success") {
        resolveFirst!(firstPublication!);
      } else {
        rejectFirst!(
          Object.assign(new Error("response lost after commit"), {
            mutationOutcome: outcome === "rejected" ? "rejected" : "unknown",
          }),
        );
      }
      if (outcome === "rejected") {
        await waitFor(() =>
          expect(getPendingCompositionSaveAttempt(routeIdentity))
            .toBeUndefined()
        );
      } else {
        await waitFor(() =>
          expect(getPendingCompositionSaveAttempt(routeIdentity)?.state)
            .toBe("uncertain")
        );
      }
      expect(screen.getByRole("dialog", { name: "Save as tool" }))
        .toBe(recoveredDialog);
      expect(within(recoveredDialog).getByRole("button", { name: "Retry" }))
        .toBeEnabled();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(navigateMock).not.toHaveBeenCalled();
      fireEvent.click(
        within(screen.getByRole("dialog", { name: "Save as tool" }))
          .getByRole("button", { name: "Retry" }),
      );
      await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(2));
      expect(compositionSaveMock.mock.calls[1]).toEqual(firstCall);
      await waitFor(() =>
        expect(navigateMock).toHaveBeenCalledWith(
          "/spaces/default/compositions/tool-1/tool-revision-1",
        )
      );
      expect(committedPublications.size).toBe(1);
    },
  );

  it.each(["success", "unknown failure", "rejected"] as const)(
    "does not clear a new route dialog after stale save %s",
    async (outcome) => {
      let resolveFirst: (
        value: Awaited<ReturnType<typeof compositionApi.save>>,
      ) => void;
      let rejectFirst: (error: unknown) => void;
      compositionSaveMock.mockImplementation(() =>
        new Promise((resolve, reject) => {
          resolveFirst = resolve;
          rejectFirst = reject;
        })
      );
      getMock.mockImplementation((_spaceId, requestedSqlId) =>
        Promise.resolve({
          id: requestedSqlId,
          name: requestedSqlId === "other-query"
            ? "Other query"
            : "Saved query",
          kind: "user-query",
          sql: "SELECT value FROM demo ORDER BY value",
          variables: [],
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-02T00:00:00Z",
          revision_id: "rev-1",
        })
      );
      queryMock.mockResolvedValue({
        columns: ["value"],
        rows: [["result"]],
        has_more: false,
      });
      render(() => <SpaceSqlRunRoute />);

      fireEvent.click(
        await screen.findByRole("button", { name: "Save as tool" }),
      );
      fireEvent.click(
        within(screen.getByRole("dialog", { name: "Save as tool" }))
          .getByRole("button", { name: "Save" }),
      );
      await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(1));

      routeControls.setSql("other-query");
      routeControls.setPath("/spaces/default/sql/other-query/run");
      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: "Save as tool" }))
          .not.toBeInTheDocument()
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "Save as tool" }),
      );
      const newDialog = screen.getByRole("dialog", { name: "Save as tool" });
      expect(screen.getByLabelText("Name", { selector: "input" }))
        .toHaveValue("Other query");

      if (outcome === "success") {
        resolveFirst!({
          composition_id: "tool-1",
          revision_id: "tool-revision-1",
          canonical_yaml: "canonical composition yaml",
          receipt: {
            command_id: "command-1",
            catalog_generation: 4,
            snapshot_id: 42,
            committed_revision_ids: ["tool-revision-1"],
            committed_at_micros: 1,
            data_file_count: 1,
          },
        });
      } else {
        rejectFirst!(
          Object.assign(new Error("response lost after commit"), {
            mutationOutcome: outcome === "rejected" ? "rejected" : "unknown",
          }),
        );
      }

      await waitFor(() => {
        expect(screen.getByRole("dialog", { name: "Save as tool" }))
          .toBe(newDialog);
        expect(screen.getByLabelText("Name", { selector: "input" }))
          .toHaveValue("Other query");
        expect(
          within(newDialog).getByRole("button", { name: "Save" }),
        ).toBeEnabled();
      });
      expect(navigateMock).not.toHaveBeenCalled();
    },
  );

  it("allows a corrected save after a recovered retry receives a deterministic validation rejection", async () => {
    const publication: Awaited<ReturnType<typeof compositionApi.save>> = {
      composition_id: "tool-1",
      revision_id: "tool-revision-1",
      canonical_yaml: "Corrected query canonical composition yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: 42,
        committed_revision_ids: ["tool-revision-1"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    };
    canonicalizeMock.mockImplementation(async (document) => ({
      document,
      canonical_yaml: `${document.name} canonical composition yaml`,
      fingerprint: String(document.name),
    }));
    compositionSaveMock.mockImplementation((_spaceId, yaml) => {
      const callCount = compositionSaveMock.mock.calls.length;
      if (callCount === 1) {
        return Promise.reject(
          Object.assign(new Error("response lost"), {
            mutationOutcome: "unknown",
          }),
        );
      }
      if (callCount === 2) {
        return Promise.reject(
          Object.assign(new Error("invalid Composition"), {
            mutationOutcome: "rejected",
            status: 422,
          }),
        );
      }
      return Promise.resolve({ ...publication, canonical_yaml: yaml });
    });

    const routeIdentity = {
      spaceId: "default",
      sqlId: "saved-query",
      routePath: "/spaces/default/sql/saved-query/run",
    };
    render(() => <SpaceSqlRunRoute />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    fireEvent.input(screen.getByLabelText("Name", { selector: "input" }), {
      target: { value: "Rejected query" },
    });
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "Save as tool" }))
        .getByRole("button", { name: "Save" }),
    );
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(getPendingCompositionSaveAttempt(routeIdentity)?.state)
        .toBe("uncertain")
    );
    const firstCall = compositionSaveMock.mock.calls[0];

    routeControls.setPath("/spaces/default/sql/saved-query");
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Save as tool" }))
        .not.toBeInTheDocument()
    );
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
    fireEvent.click(
      within(recoveredDialog).getByRole("button", { name: "Retry" }),
    );

    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(2));
    expect(compositionSaveMock.mock.calls[1]).toEqual(firstCall);
    await waitFor(() =>
      expect(getPendingCompositionSaveAttempt(routeIdentity)).toBeUndefined()
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not save this tool.",
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    const correctedDialog = screen.getByRole("dialog", {
      name: "Save as tool",
    });
    expect(within(correctedDialog).getByRole("alert")).toHaveTextContent(
      "Could not save this tool.",
    );
    expect(
      within(correctedDialog).getByLabelText("Name", { selector: "input" }),
    )
      .toHaveValue("Rejected query");
    fireEvent.input(
      within(correctedDialog).getByLabelText("Name", { selector: "input" }),
      {
        target: { value: "Corrected query" },
      },
    );
    fireEvent.click(
      within(correctedDialog).getByRole("button", { name: "Save" }),
    );
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(3));

    expect(compositionSaveMock.mock.calls[2][1]).toBe(
      "Corrected query canonical composition yaml",
    );
    expect(compositionSaveMock.mock.calls[2][2]).not.toBe(firstCall[2]);
    expect(canonicalizeMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ name: "Corrected query" }),
    );
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/compositions/tool-1/tool-revision-1",
      )
    );
    expect(getPendingCompositionSaveAttempt(routeIdentity)).toBeUndefined();
  });

  it("keeps a recovered key after a retry is rejected while the original request may commit", async () => {
    let resolveFirst: (
      value: Awaited<ReturnType<typeof compositionApi.save>>,
    ) => void;
    const publication: Awaited<ReturnType<typeof compositionApi.save>> = {
      composition_id: "tool-1",
      revision_id: "tool-revision-1",
      canonical_yaml: "canonical composition yaml",
      receipt: {
        command_id: "command-1",
        catalog_generation: 4,
        snapshot_id: 42,
        committed_revision_ids: ["tool-revision-1"],
        committed_at_micros: 1,
        data_file_count: 1,
      },
    };
    compositionSaveMock.mockImplementation((_spaceId, _yaml, _key) => {
      if (compositionSaveMock.mock.calls.length === 1) {
        return new Promise((resolve) => {
          resolveFirst = resolve;
        });
      }
      if (compositionSaveMock.mock.calls.length === 2) {
        return Promise.reject(
          Object.assign(new Error("authorization denied"), {
            mutationOutcome: "rejected",
            status: 403,
          }),
        );
      }
      return Promise.resolve(publication);
    });
    render(() => <SpaceSqlRunRoute />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Save as tool" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "Save as tool" }))
        .getByRole("button", { name: "Save" }),
    );
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(1));
    const firstCall = compositionSaveMock.mock.calls[0];
    const routeIdentity = {
      spaceId: "default",
      sqlId: "saved-query",
      routePath: "/spaces/default/sql/saved-query/run",
    };

    routeControls.setPath("/spaces/default/sql/saved-query");
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Save as tool" }))
        .not.toBeInTheDocument()
    );
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

    fireEvent.click(
      within(recoveredDialog).getByRole("button", { name: "Retry" }),
    );
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(getPendingCompositionSaveAttempt(routeIdentity)).toEqual(
        expect.objectContaining({
          state: "uncertain",
          attempt: expect.objectContaining({
            yaml: firstCall[1],
            idempotencyKey: firstCall[2],
          }),
        }),
      )
    );
    expect(within(recoveredDialog).getByRole("button", { name: "Retry" }))
      .toBeEnabled();

    resolveFirst!(publication);
    await waitFor(() =>
      expect(getPendingCompositionSaveAttempt(routeIdentity)?.state)
        .toBe("uncertain")
    );
    expect(screen.getByRole("dialog", { name: "Save as tool" }))
      .toBe(recoveredDialog);
    expect(navigateMock).not.toHaveBeenCalled();

    fireEvent.click(
      within(recoveredDialog).getByRole("button", { name: "Retry" }),
    );
    await waitFor(() => expect(compositionSaveMock).toHaveBeenCalledTimes(3));
    expect(compositionSaveMock.mock.calls[1]).toEqual(firstCall);
    expect(compositionSaveMock.mock.calls[2]).toEqual(firstCall);
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/default/compositions/tool-1/tool-revision-1",
      )
    );
    expect(getPendingCompositionSaveAttempt(routeIdentity)).toBeUndefined();
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

  it.each(["space", "sql"] as const)(
    "clears deterministic save rejection feedback when the %s route changes",
    async (changedIdentity) => {
      getMock.mockImplementation((requestedSpaceId, requestedSqlId) =>
        Promise.resolve({
          id: requestedSqlId,
          name: requestedSpaceId === "other-space"
            ? "Other space query"
            : requestedSqlId === "other-query"
            ? "Other query"
            : "Saved query",
          kind: "user-query",
          sql: "SELECT value FROM demo ORDER BY value",
          variables: [],
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-02T00:00:00Z",
          revision_id: "rev-1",
        })
      );
      queryMock.mockReset().mockResolvedValue({
        columns: ["value"],
        rows: [["result"]],
        has_more: false,
      });
      compositionSaveMock.mockRejectedValue(
        Object.assign(new Error("invalid Composition"), {
          mutationOutcome: "rejected",
          status: 422,
        }),
      );
      render(() => <SpaceSqlRunRoute />);

      fireEvent.click(
        await screen.findByRole("button", { name: "Save as tool" }),
      );
      const dialog = screen.getByRole("dialog", { name: "Save as tool" });
      fireEvent.input(
        within(dialog).getByLabelText("Name", { selector: "input" }),
        { target: { value: "Rejected query" } },
      );
      fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
      await within(dialog).findByRole("alert");
      expect(compositionSaveMock).toHaveBeenCalledTimes(1);

      fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "Could not save this tool.",
      );

      const nextName = changedIdentity === "space"
        ? "Other space query"
        : "Other query";
      if (changedIdentity === "space") {
        routeControls.setSpace("other-space");
        routeControls.setPath(
          "/spaces/other-space/sql/saved-query/run",
        );
      } else {
        routeControls.setSql("other-query");
        routeControls.setPath(
          "/spaces/default/sql/other-query/run",
        );
      }

      await waitFor(() =>
        expect(screen.queryByRole("alert")).not.toBeInTheDocument()
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "Save as tool" }),
      );
      const nextDialog = screen.getByRole("dialog", {
        name: "Save as tool",
      });
      expect(
        within(nextDialog).getByLabelText("Name", { selector: "input" }),
      ).toHaveValue(nextName);
      expect(within(nextDialog).queryByRole("alert"))
        .not.toBeInTheDocument();
    },
  );
});
