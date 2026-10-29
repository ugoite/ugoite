import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CompositionRevisionRoute from "./index";
import { setLocale } from "~/lib/i18n";

const {
  getMock,
  lintMock,
  listMock,
  evaluateMock,
  accessSnapshotMock,
  navigateMock,
  parseMock,
  routeParams,
} = vi.hoisted(() => {
  let getRouteParams = () => ({
    space_id: "space-1",
    composition_id: "comp-1",
    revision_id: "rev-1",
  });
  return {
    getMock: vi.fn(),
    lintMock: vi.fn(),
    listMock: vi.fn(),
    evaluateMock: vi.fn(),
    accessSnapshotMock: vi.fn(),
    navigateMock: vi.fn(),
    parseMock: vi.fn(),
    routeParams: {
      get: () => getRouteParams(),
      set: (get: typeof getRouteParams) => getRouteParams = get,
    },
  };
});

type RouteParams = {
  space_id: string;
  composition_id: string;
  revision_id: string;
};

let updateRouteParams: (params: RouteParams) => void;

vi.mock("@solidjs/router", () => ({
  useParams: () =>
    new Proxy({}, {
      get: (_target, property) =>
        routeParams.get()[property as keyof RouteParams],
    }),
  useNavigate: () => navigateMock,
  A: (props: { href: string; children: unknown }) => (
    <a href={props.href}>{props.children as never}</a>
  ),
}));

vi.mock("~/lib/composition-api", () => ({
  compositionApi: {
    get: (...args: unknown[]) => getMock(...args),
    lint: (...args: unknown[]) => lintMock(...args),
    list: (...args: unknown[]) => listMock(...args),
    evaluate: (...args: unknown[]) => evaluateMock(...args),
    checkAccessSnapshot: (...args: unknown[]) => accessSnapshotMock(...args),
  },
  compositionDisplayName: (name: string) => name,
}));

vi.mock("~/lib/ugoite-client", () => ({
  parseVariableGraphInput: (...args: unknown[]) => parseMock(...args),
}));

const query = (variable_id: string, ...dependencies: string[]) => ({
  variable_id,
  deferred_arguments: dependencies.map((variable_id) => ({
    argument_name: variable_id,
    variable_id,
    value_type: { scalar_type: "date", nullable: false },
  })),
  returns: {
    kind: "scalar" as const,
    value_type: { scalar_type: "integer" as const, nullable: false },
  },
  result_columns: [{
    name: "value",
    value_type: { scalar_type: "integer" as const, nullable: false },
  }],
  form_bindings: [],
  schema_fingerprint: "schema",
  query_semantic_fingerprint: `semantic-${variable_id}`,
  scalar_cardinality: "exactly_one_row" as const,
});

const evaluation = (
  evaluation_id: string,
  authorization_fingerprint: string,
  values: { rows: number; stable: number },
  computed_query_ids = ["rows", "stable"],
) => ({
  evaluation_id,
  ok: true,
  composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
  publication_fingerprint: "publication-1",
  authorization_fingerprint,
  computed_query_ids,
  variable_values: {},
  queries: [query("rows", "day"), query("stable")],
  component_bindings: [],
  dependencies: {
    variable_consumers: {
      day: { query_variable_ids: ["rows"] },
    },
  },
  query_results: Object.fromEntries(
    Object.entries(values).map(([id, value]) => [id, {
      value: {
        kind: "scalar",
        value: { kind: "integer", value: String(value) },
      },
      has_more: false,
    }]),
  ),
  diagnostics: [],
  query_diagnostics: [],
});

const graphDocument = {
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: "Report",
  spec: {
    variables: [
      {
        id: "day",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
      },
      {
        id: "rows",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: { kind: "inline_sql", sql: "SELECT 1", arguments: [] },
      },
      {
        id: "stable",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: { kind: "inline_sql", sql: "SELECT 2", arguments: [] },
      },
    ],
    components: [
      { id: "input-day", kind: "input", variable: "day" },
      { id: "metric-rows", kind: "metric", variable: "rows" },
      { id: "metric-stable", kind: "metric", variable: "stable" },
    ],
    layout: {
      kind: "flow",
      rows: [{
        id: "main",
        items: [
          { kind: "component", component: "input-day" },
          { kind: "component", component: "metric-rows" },
          { kind: "component", component: "metric-stable" },
        ],
      }],
    },
  },
};

const tableGraphDocument = {
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: "Rows",
  spec: {
    variables: [{
      id: "rows",
      kind: "query",
      returns: {
        kind: "table",
        columns: [{
          name: "label",
          value_type: { scalar_type: "string", nullable: true },
        }],
      },
      query: {
        kind: "inline_sql",
        sql: 'SELECT label FROM "G"',
        arguments: [],
      },
    }],
    components: [{ id: "rows-table", kind: "table", variable: "rows" }],
    layout: {
      kind: "flow",
      rows: [{
        id: "main",
        items: [{ kind: "component", component: "rows-table" }],
      }],
    },
  },
};

const tableWithInputGraphDocument = {
  ...tableGraphDocument,
  name: "Rows with input",
  spec: {
    ...tableGraphDocument.spec,
    variables: [
      {
        id: "day",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
        default: { kind: "date", value: "2026-10-20" },
      },
      tableGraphDocument.spec.variables[0],
      {
        id: "filtered_count",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT COUNT(*) FROM G WHERE day = $day",
          arguments: [{ name: "day", variable: "day" }],
        },
      },
    ],
    components: [
      { id: "day-input", kind: "input", variable: "day" },
      ...tableGraphDocument.spec.components,
      { id: "count-metric", kind: "metric", variable: "filtered_count" },
    ],
    layout: {
      kind: "flow",
      rows: [{
        id: "main",
        items: [
          { kind: "component", component: "day-input" },
          { kind: "component", component: "rows-table" },
          { kind: "component", component: "count-metric" },
        ],
      }],
    },
  },
};

describe("Composition saved revision graph evaluation", () => {
  beforeEach(() => {
    const [params, setParams] = createSignal<RouteParams>({
      space_id: "space-1",
      composition_id: "comp-1",
      revision_id: "rev-1",
    });
    routeParams.set(params);
    updateRouteParams = (next) => setParams(next);
    setLocale("en");
    vi.clearAllMocks();
    getMock.mockResolvedValue({ fields: { spec: "graph yaml" } });
    lintMock.mockResolvedValue({
      ok: true,
      value: { document: graphDocument },
    });
    listMock.mockResolvedValue({
      items: [{
        composition_id: "comp-1",
        revision_id: "rev-1",
      }],
    });
    accessSnapshotMock.mockResolvedValue({
      composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
      publication_fingerprint: "publication-1",
      authorization_fingerprint: "auth-1",
      valid_for_ms: 30_000,
    });
    parseMock.mockImplementation(async (_type: unknown, raw: string) =>
      raw ? { status: "valid", value: raw } : { status: "unset" }
    );
  });
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("requests only affected query variables after an input changes", async () => {
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-1", "auth-1", { rows: 1, stable: 10 }),
          evaluation_id: request.evaluation_id,
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-2", "auth-1", { rows: 2 }, ["rows"]),
          evaluation_id: request.evaluation_id,
        })
      );
    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    await screen.findByText("10");

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-20" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(2));

    expect(evaluateMock.mock.calls[0][1]).not.toHaveProperty(
      "requested_queries",
    );
    expect(evaluateMock.mock.calls[1][1].requested_queries).toEqual(["rows"]);
    expect(evaluateMock.mock.calls[1][1]).not.toHaveProperty("query_results");
    expect(screen.getByLabelText("stable")).toHaveTextContent("10");
  });

  it("fails closed when a focus-return authority check is unavailable", async () => {
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: { evaluation_id: string },
    ) =>
      Promise.resolve({
        ...evaluation(request.evaluation_id, "auth-1", { rows: 1, stable: 10 }),
      })
    );
    render(() => <CompositionRevisionRoute />);
    await screen.findByText("10");

    accessSnapshotMock.mockRejectedValueOnce(new Error("offline"));
    fireEvent(window, new Event("focus"));

    await waitFor(() => {
      expect(accessSnapshotMock).toHaveBeenCalledOnce();
      expect(screen.queryByText("10")).not.toBeInTheDocument();
    });
    expect(evaluateMock).toHaveBeenCalledOnce();
  });

  it("hides retained query values when the authority lease expires", async () => {
    vi.useFakeTimers();
    let resolveExpiredCheck!: (value: {
      composition_revision: { entry_id: string; revision_id: string };
      publication_fingerprint: string;
      authorization_fingerprint: string;
      valid_for_ms: number;
    }) => void;
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: { evaluation_id: string },
    ) =>
      Promise.resolve({
        ...evaluation(request.evaluation_id, "auth-1", { rows: 1, stable: 10 }),
      })
    );
    accessSnapshotMock.mockReturnValue(
      new Promise((resolve) => {
        resolveExpiredCheck = resolve;
      }),
    );
    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(screen.getByText("10")).toBeInTheDocument());

    await vi.advanceTimersByTimeAsync(25_000);
    expect(accessSnapshotMock).toHaveBeenCalledOnce();
    expect(screen.getByText("10")).toBeInTheDocument();
    await vi.advanceTimersByTimeAsync(5_000);

    await waitFor(() => {
      expect(screen.queryByText("10")).not.toBeInTheDocument();
    });
    resolveExpiredCheck({
      composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
      publication_fingerprint: "publication-1",
      authorization_fingerprint: "auth-1",
      valid_for_ms: 30_000,
    });
    await Promise.resolve();
    expect(screen.queryByText("10")).not.toBeInTheDocument();
  });

  it("hides values on focus while a scheduled authority check is still pending", async () => {
    vi.useFakeTimers();
    let resolveAccessCheck!: (value: {
      composition_revision: { entry_id: string; revision_id: string };
      publication_fingerprint: string;
      authorization_fingerprint: string;
      valid_for_ms: number;
    }) => void;
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: { evaluation_id: string },
    ) =>
      Promise.resolve({
        ...evaluation(request.evaluation_id, "auth-1", { rows: 1, stable: 10 }),
      })
    );
    accessSnapshotMock.mockReturnValue(
      new Promise((resolve) => {
        resolveAccessCheck = resolve;
      }),
    );
    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(screen.getByText("10")).toBeInTheDocument());
    await vi.advanceTimersByTimeAsync(25_000);
    expect(accessSnapshotMock).toHaveBeenCalledOnce();
    expect(screen.getByText("10")).toBeInTheDocument();

    fireEvent(window, new Event("focus"));
    expect(screen.queryByText("10")).not.toBeInTheDocument();

    resolveAccessCheck({
      composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
      publication_fingerprint: "publication-1",
      authorization_fingerprint: "auth-1",
      valid_for_ms: 30_000,
    });
    await waitFor(() => expect(screen.getByText("10")).toBeInTheDocument());
    expect(accessSnapshotMock).toHaveBeenCalledOnce();
  });

  it("does not restore results from an evaluation already in flight when the access check fails", async () => {
    let rejectAccessCheck!: (error: Error) => void;
    let resolveInputEvaluation!: (value: ReturnType<typeof evaluation>) => void;
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation(request.evaluation_id, "auth-1", {
            rows: 1,
            stable: 10,
          }),
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        new Promise((resolve) => {
          resolveInputEvaluation = (value) =>
            resolve({ ...value, evaluation_id: request.evaluation_id });
        })
      );
    accessSnapshotMock.mockReturnValueOnce(
      new Promise((_resolve, reject) => {
        rejectAccessCheck = reject;
      }),
    );
    render(() => <CompositionRevisionRoute />);
    await screen.findByText("10");
    fireEvent(window, new Event("focus"));
    await waitFor(() => expect(accessSnapshotMock).toHaveBeenCalledOnce());

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-20" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(2));
    rejectAccessCheck(new Error("offline"));
    await waitFor(() => {
      expect(screen.queryByText("10")).not.toBeInTheDocument();
    });
    resolveInputEvaluation(evaluation("late", "auth-1", {
      rows: 99,
      stable: 99,
    }, ["rows", "stable"]));
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.queryByText("99")).not.toBeInTheDocument();
    expect(screen.queryByText("10")).not.toBeInTheDocument();
    expect(evaluateMock).toHaveBeenCalledTimes(2);
  });

  it("discards cached results and reevaluates after the principal fingerprint changes", async () => {
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation(request.evaluation_id, "auth-1", {
            rows: 1,
            stable: 10,
          }),
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation(request.evaluation_id, "auth-2", {
            rows: 2,
            stable: 20,
          }),
        })
      );
    accessSnapshotMock.mockResolvedValueOnce({
      composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
      publication_fingerprint: "publication-1",
      authorization_fingerprint: "auth-2",
      valid_for_ms: 30_000,
    });
    render(() => <CompositionRevisionRoute />);
    await screen.findByText("10");

    fireEvent(window, new Event("focus"));

    await screen.findByText("20");
    expect(screen.queryByText("10")).not.toBeInTheDocument();
    expect(evaluateMock).toHaveBeenCalledTimes(2);
  });

  it("ignores an authority response from a route that is no longer active", async () => {
    let resolveOldCheck!: (value: {
      composition_revision: { entry_id: string; revision_id: string };
      publication_fingerprint: string;
      authorization_fingerprint: string;
      valid_for_ms: number;
    }) => void;
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: {
        evaluation_id: string;
        target: { revision_id: string };
      },
    ) =>
      Promise.resolve({
        ...evaluation(
          request.evaluation_id,
          "auth-1",
          request.target.revision_id === "rev-1"
            ? { rows: 1, stable: 10 }
            : { rows: 2, stable: 20 },
        ),
        composition_revision: {
          entry_id: "comp-1",
          revision_id: request.target.revision_id,
        },
      })
    );
    accessSnapshotMock.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOldCheck = resolve;
      }),
    );
    render(() => <CompositionRevisionRoute />);
    await screen.findByText("10");
    fireEvent(window, new Event("focus"));
    await waitFor(() => expect(accessSnapshotMock).toHaveBeenCalledOnce());

    updateRouteParams({
      space_id: "space-1",
      composition_id: "comp-1",
      revision_id: "rev-2",
    });
    await screen.findByText("20");
    resolveOldCheck({
      composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
      publication_fingerprint: "publication-1",
      authorization_fingerprint: "auth-1",
      valid_for_ms: 30_000,
    });

    expect(screen.getByText("20")).toBeInTheDocument();
    expect(screen.queryByText("10")).not.toBeInTheDocument();
    expect(evaluateMock).toHaveBeenCalledTimes(2);
  });

  it("immediately hides stale results and never evaluates invalid raw input", async () => {
    let resolveChangedEvaluation!: (
      value: ReturnType<typeof evaluation>,
    ) => void;
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation(request.evaluation_id, "auth-1", {
            rows: 1,
            stable: 10,
          }),
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        new Promise((resolve) => {
          resolveChangedEvaluation = (value) =>
            resolve({ ...value, evaluation_id: request.evaluation_id });
        })
      );
    evaluateMock.mockImplementation((
      _space: string,
      request: { evaluation_id: string },
    ) => Promise.resolve(evaluation(request.evaluation_id, "auth-1", {}, [])));
    parseMock.mockImplementation(async (_type: unknown, raw: string) =>
      raw === "bad"
        ? { status: "invalid" }
        : raw
        ? { status: "valid", value: raw }
        : { status: "unset" }
    );
    const integerInputDocument = structuredClone(graphDocument);
    const dayInput = integerInputDocument.spec.variables[0] as {
      value_type: { scalar_type: string };
    };
    dayInput.value_type.scalar_type = "integer";
    lintMock.mockResolvedValue({
      ok: true,
      value: { document: integerInputDocument },
    });
    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    await screen.findByText("1");

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-21" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(2));
    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "bad" },
    });

    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for input…",
    );
    resolveChangedEvaluation(evaluation("stale", "auth-1", {
      rows: 99,
      stable: 10,
    }, ["rows"]));
    const input = screen.getByLabelText("day");
    await screen.findByText("Enter a valid value.");
    expect(input).toHaveAttribute("aria-invalid", "true");
    await new Promise((resolve) => setTimeout(resolve, 300));

    expect(evaluateMock).toHaveBeenCalledTimes(3);
    expect(evaluateMock.mock.calls[2][1]).toMatchObject({
      requested_queries: [],
      variables: {},
      unset_variables: ["day"],
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for input…",
    );
    expect(screen.getByLabelText("stable")).toHaveTextContent("10");
  });

  it("drops late evaluations when a dynamic route changes Space", async () => {
    let resolveSpaceOneEvaluation!: () => void;
    let resolveSpaceTwoDocument!: () => void;
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation(request.evaluation_id, "auth-1", {
            rows: 1,
            stable: 10,
          }),
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        new Promise((resolve) => {
          resolveSpaceOneEvaluation = () =>
            resolve({
              ...evaluation(request.evaluation_id, "auth-1", {
                rows: 99,
                stable: 999,
              }),
            });
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation(request.evaluation_id, "auth-2", {
            rows: 2,
            stable: 20,
          }),
        })
      );
    getMock.mockImplementation((spaceId: string) => {
      if (spaceId === "space-2") {
        return new Promise((resolve) => {
          resolveSpaceTwoDocument = () =>
            resolve({ fields: { spec: "graph yaml" } });
        });
      }
      return Promise.resolve({ fields: { spec: "graph yaml" } });
    });

    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    await screen.findByText("10");

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-21" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(2));

    updateRouteParams({
      space_id: "space-2",
      composition_id: "comp-1",
      revision_id: "rev-1",
    });
    await waitFor(() =>
      expect(getMock).toHaveBeenCalledWith("space-2", "comp-1", "rev-1")
    );
    expect(screen.queryByText("10")).not.toBeInTheDocument();

    resolveSpaceOneEvaluation();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByText("99")).not.toBeInTheDocument();
    expect(screen.queryByText("999")).not.toBeInTheDocument();

    resolveSpaceTwoDocument();
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(3));
    await screen.findByText("20");
    expect(screen.queryByText("999")).not.toBeInTheDocument();
  });

  it("ignores late input parsing from a previous Space with the same input ID", async () => {
    let finishOldParse!: (value: unknown) => void;
    let finishNewParse!: (value: unknown) => void;
    parseMock
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          finishOldParse = resolve;
        })
      )
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          finishNewParse = resolve;
        })
      );
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: { evaluation_id: string },
    ) =>
      Promise.resolve(
        evaluation(request.evaluation_id, "auth-1", { rows: 1, stable: 10 }),
      )
    );
    render(() => <CompositionRevisionRoute />);
    await screen.findByLabelText("day");
    fireEvent.input(screen.getByLabelText("day"), { target: { value: "old" } });
    updateRouteParams({
      space_id: "space-2",
      composition_id: "comp-1",
      revision_id: "rev-1",
    });
    await waitFor(() =>
      expect(getMock).toHaveBeenCalledWith("space-2", "comp-1", "rev-1")
    );
    await waitFor(() => expect(screen.getByLabelText("day")).toHaveValue(""));
    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-23" },
    });
    finishNewParse({ status: "valid", value: "2026-10-23" });
    await waitFor(() =>
      expect(evaluateMock.mock.calls.at(-1)?.[1].variables).toEqual({
        day: "2026-10-23",
      })
    );
    finishOldParse({ status: "valid", value: "2026-01-01" });
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(evaluateMock.mock.calls.at(-1)?.[1].variables).toEqual({
      day: "2026-10-23",
    });
    expect(screen.getByLabelText("day")).toHaveValue("2026-10-23");
  });

  it("discards page history and restarts without a cursor when authority changes", async () => {
    lintMock.mockResolvedValue({
      ok: true,
      value: { document: tableGraphDocument },
    });
    let finishRefresh!: (value: unknown) => void;
    const result = (id: string, auth: string, text: string, next?: string) => ({
      ...evaluation(id, auth, {}, ["rows"]),
      query_results: {
        rows: {
          value: {
            kind: "table",
            columns: tableGraphDocument.spec.variables[0].returns.columns,
            rows: [[{ kind: "string", value: text }]],
          },
          has_more: Boolean(next),
          ...(next ? { next } : {}),
        },
      },
    });
    evaluateMock
      .mockImplementationOnce((
        _space: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve(
          result(request.evaluation_id, "auth-1", "old page", "cursor"),
        )
      )
      .mockImplementationOnce((
        _space: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve(result(request.evaluation_id, "auth-2", "changed page"))
      )
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          finishRefresh = resolve;
        })
      );
    render(() => <CompositionRevisionRoute />);
    await screen.findByText("old page");
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(3));
    expect(evaluateMock.mock.calls[2][1]).not.toHaveProperty("page");
    expect(evaluateMock.mock.calls[2][1]).not.toHaveProperty(
      "requested_queries",
    );
    expect(screen.queryByText("old page")).not.toBeInTheDocument();
    finishRefresh(
      result(
        evaluateMock.mock.calls[2][1].evaluation_id,
        "auth-2",
        "fresh page",
      ),
    );
    await screen.findByText("fresh page");
    expect(screen.queryByRole("button", { name: "Previous", hidden: false }))
      .not.toBeInTheDocument();
    expect(screen.queryByText("old page")).not.toBeInTheDocument();
  });

  it("renders the typed table result returned for its query variable", async () => {
    lintMock.mockResolvedValue({
      ok: true,
      value: { document: tableGraphDocument },
    });
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: {
        evaluation_id: string;
        page?: { variable_id: string; cursor: string };
      },
    ) =>
      Promise.resolve({
        evaluation_id: request.evaluation_id,
        ok: true,
        composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
        publication_fingerprint: "publication-1",
        authorization_fingerprint: "auth-1",
        computed_query_ids: ["rows"],
        variable_values: {},
        queries: [{
          variable_id: "rows",
          deferred_arguments: [],
          returns: {
            kind: "table",
            columns: [{
              name: "label",
              value_type: { scalar_type: "string", nullable: true },
            }],
          },
          result_columns: [{
            name: "label",
            value_type: { scalar_type: "string", nullable: true },
          }],
          form_bindings: [{ relation: "G", form_id: "form-1" }],
          schema_fingerprint: "schema-1",
          query_semantic_fingerprint: "semantic-rows",
        }],
        component_bindings: [{
          component_id: "rows-table",
          kind: "table",
          variable_id: "rows",
        }],
        dependencies: { variable_consumers: {} },
        query_results: {
          rows: {
            value: {
              kind: "table",
              columns: [{
                name: "label",
                value_type: { scalar_type: "string", nullable: true },
              }],
              rows: [
                ...(request.page
                  ? [[{ kind: "string" as const, value: "next row" }]]
                  : [
                    [{ kind: "string" as const, value: "alpha row" }],
                    [{ kind: "string" as const, value: "beta row" }],
                  ]),
              ],
            },
            has_more: !request.page,
            ...(!request.page ? { next: "rows-cursor-1" } : {}),
          },
        },
      })
    );

    render(() => <CompositionRevisionRoute />);

    const table = await screen.findByRole("table", { name: "rows results" });
    expect(table).toHaveTextContent("alpha row");
    expect(table).toHaveTextContent("beta row");
    expect(evaluateMock.mock.calls[0][1].target).toEqual({
      kind: "saved_revision",
      composition_id: "comp-1",
      revision_id: "rev-1",
    });
    expect(evaluateMock.mock.calls[0][1]).not.toHaveProperty("sources");

    await fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(2));
    expect(evaluateMock.mock.calls[1][1].page).toEqual({
      variable_id: "rows",
      cursor: "rows-cursor-1",
    });
    await waitFor(() => expect(table).toHaveTextContent("next row"));
    expect(table).not.toHaveTextContent("alpha row");
  });

  it("keeps an unrelated table continuation after an input changes", async () => {
    lintMock.mockResolvedValue({
      ok: true,
      value: { document: tableWithInputGraphDocument },
    });
    const tableQuery = {
      variable_id: "rows",
      deferred_arguments: [],
      returns: {
        kind: "table" as const,
        columns: [{
          name: "label",
          value_type: { scalar_type: "string" as const, nullable: true },
        }],
      },
      result_columns: [{
        name: "label",
        value_type: { scalar_type: "string" as const, nullable: true },
      }],
      form_bindings: [],
      schema_fingerprint: "schema-rows",
      query_semantic_fingerprint: "semantic-rows",
    };
    const countQuery = {
      ...query("filtered_count", "day"),
    };
    const result = (
      request: { evaluation_id: string; page?: { variable_id: string } },
      computedQueryIds: string[],
      labels: string[],
      hasMore: boolean,
      next: string | undefined,
      count?: number,
    ) => ({
      evaluation_id: request.evaluation_id,
      ok: true,
      composition_revision: { entry_id: "comp-1", revision_id: "rev-1" },
      publication_fingerprint: "publication-1",
      authorization_fingerprint: "auth-1",
      computed_query_ids: computedQueryIds,
      variable_values: {},
      queries: [tableQuery, countQuery],
      component_bindings: [
        { component_id: "day-input", kind: "input", variable_id: "day" },
        { component_id: "rows-table", kind: "table", variable_id: "rows" },
        {
          component_id: "count-metric",
          kind: "metric",
          variable_id: "filtered_count",
        },
      ],
      dependencies: {
        variable_consumers: { day: { query_variable_ids: ["filtered_count"] } },
      },
      query_results: {
        ...(computedQueryIds.includes("rows")
          ? {
            rows: {
              value: {
                kind: "table",
                columns: tableQuery.returns.columns,
                rows: labels.map((label) => [{ kind: "string", value: label }]),
              },
              has_more: hasMore,
              ...(next ? { next } : {}),
            },
          }
          : {}),
        ...(count !== undefined
          ? {
            filtered_count: {
              value: {
                kind: "scalar",
                value: { kind: "integer", value: count },
              },
              has_more: false,
            },
          }
          : {}),
      },
    });
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: {
        evaluation_id: string;
        page?: { variable_id: string; cursor: string };
        requested_queries?: string[];
      },
    ) => {
      if (request.page?.variable_id === "rows") {
        return Promise.resolve(
          request.page.cursor === "rows-cursor-1"
            ? result(request, ["rows"], ["gamma row"], true, "rows-cursor-2")
            : result(request, ["rows"], ["delta row"], false, undefined),
        );
      }
      if (request.requested_queries) {
        return Promise.resolve(
          result(request, ["filtered_count"], [], false, undefined, 2),
        );
      }
      return Promise.resolve(
        result(
          request,
          ["rows", "filtered_count"],
          ["alpha row", "beta row"],
          true,
          "rows-cursor-1",
          1,
        ),
      );
    });

    render(() => <CompositionRevisionRoute />);
    const table = await screen.findByRole("table", { name: "rows results" });
    expect(table).toHaveTextContent("alpha row");
    await fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(table).toHaveTextContent("gamma row"));

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-21" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(3));
    expect(evaluateMock.mock.calls[2][1].requested_queries).toEqual([
      "filtered_count",
    ]);

    await fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(4));
    expect(evaluateMock.mock.calls[3][1].page).toEqual({
      variable_id: "rows",
      cursor: "rows-cursor-2",
    });
    await waitFor(() => expect(table).toHaveTextContent("delta row"));
  });

  it("drops cached values and requests the full graph after an ACL fingerprint changes", async () => {
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-1", "auth-1", { rows: 1, stable: 10 }),
          evaluation_id: request.evaluation_id,
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-2", "auth-2", { rows: 2 }, ["rows"]),
          evaluation_id: request.evaluation_id,
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-3", "auth-2", { rows: 2, stable: 20 }),
          evaluation_id: request.evaluation_id,
        })
      );
    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    await screen.findByText("10");

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-20" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(3));

    expect(evaluateMock.mock.calls[1][1].requested_queries).toEqual(["rows"]);
    expect(evaluateMock.mock.calls[2][1]).not.toHaveProperty(
      "requested_queries",
    );
    expect(screen.getByLabelText("stable")).toHaveTextContent("20");
  });

  it("retries affected queries with unchanged valid input after a failed evaluation", async () => {
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-1", "auth-1", { rows: 1, stable: 10 }),
          evaluation_id: request.evaluation_id,
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-2", "auth-1", { rows: 0, stable: 10 }, []),
          evaluation_id: request.evaluation_id,
          ok: false,
          query_results: {},
          diagnostics: [{ code: "query_failed" }],
          query_diagnostics: [{ code: "sql_error", variable_id: "rows" }],
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-3", "auth-1", { rows: 1, stable: 10 }, ["rows"]),
          evaluation_id: request.evaluation_id,
        })
      );
    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-20" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(2));

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-20" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(3));

    expect(evaluateMock.mock.calls[2][1].requested_queries).toEqual(["rows"]);
  });

  it("does not retry a required-unset result without an evaluation identity", async () => {
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation(request.evaluation_id, "auth-1", {
            rows: 1,
            stable: 10,
          }),
        })
      )
      .mockImplementation((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          evaluation_id: request.evaluation_id,
          ok: false,
          variable_values: {},
          queries: [],
          component_bindings: [],
          dependencies: { variable_consumers: {} },
          query_results: {},
          computed_query_ids: [],
          diagnostics: [{ code: "variable_missing", variable_id: "day" }],
          query_diagnostics: [],
        })
      );
    render(() => <CompositionRevisionRoute />);
    await screen.findByText("10");

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(2));
    expect(evaluateMock.mock.calls[1][1].unset_variables).toEqual(["day"]);
    await new Promise((resolve) => setTimeout(resolve, 300));

    expect(evaluateMock).toHaveBeenCalledTimes(2);
  });

  it("retries a partial failure when its evaluation identity changed", async () => {
    evaluateMock
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-1", "auth-1", { rows: 1, stable: 10 }),
          evaluation_id: request.evaluation_id,
        })
      )
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) => {
        const partial = evaluation(
          "partial",
          "auth-2",
          { rows: 2, stable: 10 },
          ["rows"],
        );
        partial.query_results = {};
        return Promise.resolve({
          ...partial,
          evaluation_id: request.evaluation_id,
          ok: false,
          diagnostics: [{ code: "query_failed" }],
          query_diagnostics: [{ code: "sql_error", variable_id: "rows" }],
        });
      })
      .mockImplementationOnce((
        _spaceId: string,
        request: { evaluation_id: string },
      ) =>
        Promise.resolve({
          ...evaluation("eval-3", "auth-2", { rows: 2, stable: 20 }),
          evaluation_id: request.evaluation_id,
        })
      );
    render(() => <CompositionRevisionRoute />);
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    await screen.findByText("10");

    fireEvent.input(screen.getByLabelText("day"), {
      target: { value: "2026-10-20" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(3));

    expect(evaluateMock.mock.calls[1][1].requested_queries).toEqual(["rows"]);
    expect(evaluateMock.mock.calls[2][1]).not.toHaveProperty(
      "requested_queries",
    );
    await screen.findByText("20");
    expect(screen.queryByText("10")).not.toBeInTheDocument();
  });
  it("keeps an independent input branch live while another input is invalid", async () => {
    const document = structuredClone(graphDocument);
    document.spec.variables[0] = {
      id: "day",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      required: true,
      default: { kind: "integer", value: "1" },
    } as never;
    document.spec.variables.push(
      {
        id: "other",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: "3" },
      } as never,
    );
    for (const [id, input] of [["rows", "day"], ["stable", "other"]]) {
      const node = document.spec.variables.find((node) => node.id === id) as {
        query: unknown;
      };
      node.query = {
        kind: "inline_sql",
        sql: `SELECT $${input} AS value`,
        arguments: [{ name: input, variable: input }],
      };
    }
    document.spec.components.push({
      id: "other-input",
      kind: "input",
      variable: "other",
    });
    document.spec.layout.rows[0].items.push({
      kind: "component",
      component: "other-input",
    });
    lintMock.mockResolvedValue({ ok: true, value: { document } });
    parseMock.mockImplementation(async (_type, raw) =>
      raw === "bad" ? { status: "invalid" } : { status: "valid", value: raw }
    );
    evaluateMock.mockImplementation(async (_space, request) => {
      const ids = request.requested_queries ?? ["rows", "stable"];
      const values = Object.fromEntries(
        ids.map((id: string) => [
          id,
          id === "rows"
            ? request.variables.day ?? 1
            : request.variables.other ?? 3,
        ]),
      ) as { rows: number; stable: number };
      return evaluation(request.evaluation_id, "auth-1", values, ids);
    });
    render(() => <CompositionRevisionRoute />);
    await screen.findByText("3");
    fireEvent.input(screen.getByLabelText("day"), { target: { value: "bad" } });
    await screen.findByText("Enter a valid value.");
    fireEvent.input(screen.getByLabelText("other"), { target: { value: "7" } });
    await screen.findByText("7");
    expect(evaluateMock.mock.calls.at(-1)?.[1]).toMatchObject({
      requested_queries: ["stable"],
      variables: { other: "7" },
      unset_variables: ["day"],
    });
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for input");
    fireEvent.input(screen.getByLabelText("day"), { target: { value: "2" } });
    await screen.findByText("2");
    expect(screen.getByLabelText("stable")).toHaveTextContent("7");
    expect(evaluateMock.mock.calls.at(-1)?.[1].requested_queries).toEqual([
      "rows",
    ]);
  });
});
