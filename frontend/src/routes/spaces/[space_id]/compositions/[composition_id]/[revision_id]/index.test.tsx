import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import CompositionRevisionRoute from "./index";
import { setLocale } from "~/lib/i18n";

const { getMock, lintMock, listMock, evaluateMock, navigateMock, parseMock } =
  vi.hoisted(() => ({
    getMock: vi.fn(),
    lintMock: vi.fn(),
    listMock: vi.fn(),
    evaluateMock: vi.fn(),
    navigateMock: vi.fn(),
    parseMock: vi.fn(),
  }));

vi.mock("@solidjs/router", () => ({
  useParams: () => ({
    space_id: "space-1",
    composition_id: "comp-1",
    revision_id: "rev-1",
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
      value: { kind: "scalar", value: { kind: "integer", value } },
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
    parseMock.mockImplementation(async (_type: unknown, raw: string) =>
      raw ? { status: "valid", value: raw } : { status: "unset" }
    );
  });
  afterEach(() => cleanup());

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

    expect(evaluateMock).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for input…",
    );
    expect(screen.getByLabelText("stable")).toHaveTextContent("10");
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
        ...(computedQueryIds.includes("rows") ? {
          rows: {
            value: {
              kind: "table",
              columns: tableQuery.returns.columns,
              rows: labels.map((label) => [{ kind: "string", value: label }]),
            },
            has_more: hasMore,
            ...(next ? { next } : {}),
          },
        } : {}),
        ...(count !== undefined ? {
          filtered_count: {
            value: { kind: "scalar", value: { kind: "integer", value: count } },
            has_more: false,
          },
        } : {}),
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
        return Promise.resolve(result(request, ["filtered_count"], [], false, undefined, 2));
      }
      return Promise.resolve(
        result(request, ["rows", "filtered_count"], ["alpha row", "beta row"], true, "rows-cursor-1", 1),
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

  it("recomputes affected queries when an input returns to its value after a failed evaluation", async () => {
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
      target: { value: "" },
    });
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledTimes(3));

    expect(evaluateMock.mock.calls[2][1].requested_queries).toEqual(["rows"]);
  });
});
