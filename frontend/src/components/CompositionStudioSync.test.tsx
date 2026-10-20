import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
import {
  CompositionStudio,
  type CompositionStudioSaveMode,
} from "./CompositionStudio";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

const {
  authSessionMock,
  navigateMock,
  formsMock,
  savedSqlMock,
  previewGraphMock,
  evaluateMock,
  canonicalizeMock,
  parseInputMock,
  saveMock,
  routePathMock,
} = vi.hoisted(
  () => ({
    authSessionMock: vi.fn(),
    navigateMock: vi.fn(),
    formsMock: vi.fn(),
    savedSqlMock: vi.fn(),
    previewGraphMock: vi.fn(),
    evaluateMock: vi.fn(),
    canonicalizeMock: vi.fn(),
    parseInputMock: vi.fn(),
    saveMock: vi.fn(),
    routePathMock: { value: "/spaces/space-1/compositions/new" },
  }),
);

vi.mock("@solidjs/router", () => ({
  useLocation: () => ({
    get pathname() {
      return routePathMock.value;
    },
  }),
  useNavigate: () => navigateMock,
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children as never}</a>
  ),
}));

vi.mock("~/lib/ugoite-client", () => ({
  authApi: { getSession: (...args: unknown[]) => authSessionMock(...args) },
  formApi: { list: (...args: unknown[]) => formsMock(...args) },
  sqlApi: { list: (...args: unknown[]) => savedSqlMock(...args) },
  parseVariableGraphInput: (...args: unknown[]) => parseInputMock(...args),
}));

vi.mock("~/lib/composition-api", () => ({
  compositionApi: {
    previewGraph: (...args: unknown[]) => previewGraphMock(...args),
    evaluate: (...args: unknown[]) => evaluateMock(...args),
    canonicalizeDocument: (...args: unknown[]) => canonicalizeMock(...args),
    save: (...args: unknown[]) => saveMock(...args),
  },
}));

vi.mock("~/components/SQLWorkspace", () => ({
  SQLWorkspace: (props: {
    sql: string;
    onSqlChange?: (value: string) => void;
    onPreview?: () => void;
    previewDisabled?: boolean;
    preview?: { status: string; isCurrent: boolean };
  }) => (
    <>
      <textarea
        aria-label="SQL"
        value={props.sql}
        onInput={(event) => props.onSqlChange?.(event.currentTarget.value)}
      />
      <button
        type="button"
        disabled={props.previewDisabled}
        onClick={() => props.onPreview?.()}
      >
        Run query
      </button>
      <span data-testid="query-preview-status">{props.preview?.status}</span>
    </>
  ),
}));

const draftWithTable = (): CompositionGraphDraft => ({
  ...createCompositionGraphDraft("Report"),
  variables: [{
    id: "rows",
    kind: "query",
    returns: { kind: "table", columns: [] },
    query: {
      kind: "inline_sql",
      sql: 'SELECT * FROM "form_1"',
      arguments: [],
      form_bindings: [],
      schema_fingerprint: "",
    },
  }],
});

const renderedDraftWithTable = (): CompositionGraphDraft => ({
  ...draftWithTable(),
  components: [{ id: "table_1", kind: "table", variable: "rows" }],
  layout: {
    kind: "flow",
    rows: [{
      id: "main",
      items: [{ kind: "component", component: "table_1" }],
    }],
  },
});

const plannedQuery = (
  yaml: string,
  returns:
    | { kind: "scalar"; value_type: { scalar_type: string; nullable: boolean } }
    | {
      kind: "table";
      columns: Array<{
        name: string;
        value_type: { scalar_type: string; nullable: boolean };
      }>;
    },
) => {
  const document = JSON.parse(yaml);
  const query = document.spec.variables.filter((node: { kind: string }) =>
    node.kind === "query"
  ).at(-1);
  const resultColumns = returns.kind === "table"
    ? returns.columns
    : [{ name: "value", value_type: returns.value_type }];
  return {
    ok: true,
    plan: {
      queries: [{
        variable_id: query.id,
        deferred_arguments: [],
        returns,
        result_columns: resultColumns,
        form_bindings: [],
        schema_fingerprint: "schema-fingerprint",
        query_semantic_fingerprint: "semantic-fingerprint",
        ...(returns.kind === "scalar"
          ? { scalar_cardinality: "exactly_one_row" }
          : {}),
      }],
    },
  };
};

const renderStudio = (
  initialDraft = draftWithTable(),
  saveMode: CompositionStudioSaveMode = { kind: "create" },
) =>
  render(() => (
    <CompositionStudio
      spaceId="space-1"
      initialDraft={initialDraft}
      saveMode={saveMode}
      backHref="/spaces/space-1/compositions"
      backLabel="Back to compositions"
    />
  ));

describe("CompositionStudio graph surface", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    routePathMock.value = "/spaces/space-1/compositions/new";
    parseInputMock.mockImplementation(async (_type: unknown, raw: string) =>
      raw === "bad"
        ? { status: "invalid" }
        : raw
        ? { status: "valid", value: Number(raw) }
        : { status: "unset" }
    );
    authSessionMock.mockResolvedValue({
      authenticated: true,
      account: { account_id: "editor-1", display_name: "Editor" },
    });
    formsMock.mockResolvedValue([]);
    savedSqlMock.mockResolvedValue([]);
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "table",
        columns: [{
          name: "amount",
          value_type: { scalar_type: "float", nullable: false },
        }],
      })
    );
    canonicalizeMock.mockImplementation(async (document: unknown) => ({
      canonical_yaml: JSON.stringify(document),
    }));
    saveMock.mockResolvedValue({
      composition_id: "composition-1",
      revision_id: "revision-1",
    });
    evaluateMock.mockImplementation((
      _spaceId: string,
      request: { evaluation_id: string },
    ) =>
      Promise.resolve({
        evaluation_id: request.evaluation_id,
        ok: true,
        query_results: {
          query_1: {
            value: {
              kind: "scalar",
              value: { kind: "integer", value: 1 },
            },
            has_more: false,
          },
        },
        query_diagnostics: [],
        diagnostics: [],
      })
    );
  });
  afterEach(() => cleanup());

  it("offers only Design and Variables and inserts a Table without opening an Inspector", async () => {
    renderStudio();
    expect(await screen.findByRole("tab", { name: "Design" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Variables" })).toBeVisible();
    expect(screen.queryByRole("tab", { name: "Data" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "rows" }),
    );

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("button", { name: "table, rows" })).toBeVisible();
    expect(screen.queryByRole("button", { name: /Change variable/ }))
      .toBeNull();
  });

  it("creates and places an Input from Design when the graph has no inputs", async () => {
    previewGraphMock.mockResolvedValueOnce({
      ok: true,
      plan: { queries: [] },
    });
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(
      await screen.findByRole("button", { name: "Add to design" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Input" }));

    expect(await screen.findByRole("button", { name: "input, input_1" }))
      .toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const saveCall = screen.getByRole("button", { name: "Save" });
    expect(saveCall).toBeEnabled();
    fireEvent.click(saveCall);
    await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledOnce());

    const spec = (canonicalizeMock.mock.calls[0][0] as {
      spec: CompositionGraphDraft;
    }).spec;
    expect(spec.variables).toContainEqual(expect.objectContaining({
      id: "input_1",
      kind: "input",
    }));
    expect(spec.components).toContainEqual({
      id: "input_1",
      kind: "input",
      variable: "input_1",
    });
    expect(spec.layout.rows.flatMap((row) => row.items)).toContainEqual({
      kind: "component",
      component: "input_1",
    });
  });

  it("locks the captured draft while Save is validating it", async () => {
    renderStudio(renderedDraftWithTable());
    const save = await screen.findByRole("button", { name: "Save" });
    await waitFor(() => expect(save).toBeEnabled());
    let finishPreview!: (result: ReturnType<typeof plannedQuery>) => void;
    previewGraphMock.mockClear();
    previewGraphMock.mockImplementationOnce(() =>
      new Promise<ReturnType<typeof plannedQuery>>((resolve) => {
        finishPreview = resolve;
      })
    );

    fireEvent.click(save);
    await waitFor(() => expect(previewGraphMock).toHaveBeenCalledOnce());
    const root = document.querySelector(".compositionGraphStudioRoot");
    const editor = document.querySelector(".compositionGraphStudioEditor");
    expect(root).toHaveAttribute("aria-busy", "true");
    expect((editor as HTMLElement).inert).toBe(true);
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue(
      "Report",
    );

    finishPreview(plannedQuery(previewGraphMock.mock.calls[0][1], {
      kind: "table",
      columns: [{
        name: "amount",
        value_type: { scalar_type: "float", nullable: false },
      }],
    }));
    await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledOnce());
    expect(
      (canonicalizeMock.mock.calls[0][0] as { name: string }).name,
    ).toBe("Report");
    expect((editor as HTMLElement).inert).toBe(false);
  });

  it("retries an uncertain save with the same payload and idempotency key", async () => {
    saveMock
      .mockRejectedValueOnce(
        Object.assign(new Error("response lost"), {
          mutationOutcome: "unknown",
        }),
      )
      .mockResolvedValueOnce({
        composition_id: "composition-1",
        revision_id: "revision-1",
      });
    const rendered = renderStudio(renderedDraftWithTable());
    fireEvent.click(await screen.findByRole("button", { name: "Save" }));

    await screen.findByRole("button", { name: "Retry" });
    expect(
      (document.querySelector(
        ".compositionGraphStudioEditor",
      ) as HTMLElement).inert,
    ).toBe(true);
    rendered.unmount();
    renderStudio(renderedDraftWithTable());
    const retry = await screen.findByRole("button", { name: "Retry" });
    expect(
      (document.querySelector(
        ".compositionGraphStudioEditor",
      ) as HTMLElement).inert,
    ).toBe(true);
    fireEvent.click(retry);

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    expect(saveMock.mock.calls[1]).toEqual(saveMock.mock.calls[0]);
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/space-1/compositions/composition-1/revision-1",
      )
    );
  });

  it("keeps a remounted editor locked while a save is in flight", async () => {
    let rejectSave!: (error: unknown) => void;
    saveMock.mockImplementationOnce(() =>
      new Promise((_resolve, reject) => rejectSave = reject)
    );

    const firstVisit = renderStudio(renderedDraftWithTable());
    fireEvent.click(await screen.findByRole("button", { name: "Save" }));
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    const originalCall = saveMock.mock.calls[0];

    firstVisit.unmount();
    renderStudio(renderedDraftWithTable());
    const save = await screen.findByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();

    rejectSave(
      Object.assign(new Error("response lost"), {
        mutationOutcome: "unknown",
      }),
    );
    const retry = await screen.findByRole("button", { name: "Retry" });
    fireEvent.click(retry);

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    expect(saveMock.mock.calls[1]).toEqual(originalCall);
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/space-1/compositions/composition-1/revision-1",
      )
    );
  });

  it("keeps an in-flight save bound to the route target captured before canonicalization", async () => {
    let finishCanonicalize!: (result: { canonical_yaml: string }) => void;
    canonicalizeMock.mockImplementationOnce(() =>
      new Promise((resolve) => finishCanonicalize = resolve)
    );
    saveMock.mockRejectedValueOnce(
      Object.assign(new Error("response lost"), {
        mutationOutcome: "unknown",
      }),
    ).mockResolvedValueOnce({
      composition_id: "composition-1",
      revision_id: "revision-1",
    });
    const [saveMode, setSaveMode] = createSignal<CompositionStudioSaveMode>({
      kind: "create",
    });
    render(() => (
      <CompositionStudio
        spaceId="space-1"
        initialDraft={renderedDraftWithTable()}
        saveMode={saveMode()}
        backHref="/spaces/space-1/compositions"
        backLabel="Back to compositions"
      />
    ));

    fireEvent.click(await screen.findByRole("button", { name: "Save" }));
    await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledOnce());
    routePathMock.value = "/spaces/space-1/compositions/composition-2/edit";
    setSaveMode({
      kind: "update",
      compositionId: "composition-2",
      baseRevisionId: "revision-2",
    });
    finishCanonicalize({
      canonical_yaml: JSON.stringify(canonicalizeMock.mock.calls[0][0]),
    });

    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    expect(saveMock.mock.calls[0]).toEqual([
      "space-1",
      expect.any(String),
      expect.any(String),
      undefined,
    ]);
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull()
    );

    routePathMock.value = "/spaces/space-1/compositions/new";
    setSaveMode({ kind: "create" });
    const retry = await screen.findByRole("button", { name: "Retry" });
    fireEvent.click(retry);
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    expect(saveMock.mock.calls[1]).toEqual(saveMock.mock.calls[0]);
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith(
        "/spaces/space-1/compositions/composition-1/revision-1",
      )
    );
  });

  it("edits the shared graph through the Variables mode", async () => {
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));

    expect(screen.getByRole("textbox", { name: "Variable ID" })).toHaveValue(
      "input_1",
    );
    expect(
      screen.queryByText("SQL", { selector: ".compositionVariableRailItem" }),
    ).toBeNull();
  });

  it("writes inline SQL editor changes back to the graph node", async () => {
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "SQL" }));

    const editor = screen.getByRole("textbox", { name: "SQL" });
    expect(editor).toHaveValue("SELECT 1");
    fireEvent.input(editor, { target: { value: "SELECT 2" } });
    expect(editor).toHaveValue("SELECT 2");
  });

  it("keeps Save disabled for an invalid literal after another variable is valid", async () => {
    const draft = renderedDraftWithTable();
    draft.variables.unshift(
      {
        id: "first",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 1 },
      },
      {
        id: "second",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 2 },
      },
    );
    renderStudio(draft);
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));

    fireEvent.click(screen.getByRole("button", { name: "first" }));
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();
    fireEvent.input(screen.getByRole("textbox", { name: "Value" }), {
      target: { value: "bad" },
    });
    expect(save).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "second" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Value" }), {
      target: { value: "3" },
    });
    await waitFor(() => expect(parseInputMock).toHaveBeenCalledTimes(2));
    await Promise.resolve();

    expect(save).toBeDisabled();
    fireEvent.click(save);
    expect(canonicalizeMock).not.toHaveBeenCalled();
  });

  it("discards a query preview response when its argument binding changes", async () => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [
      {
        id: "threshold",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 1 },
      },
      {
        id: "replacement",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 2 },
      },
      {
        id: "query_1",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT $limit",
          arguments: [{ name: "limit", variable: "threshold" }],
        },
      },
    ];
    let resolveEvaluation!: (value: unknown) => void;
    evaluateMock.mockImplementationOnce((
      _spaceId: string,
      request: { evaluation_id: string },
    ) =>
      new Promise((resolve) => {
        resolveEvaluation = () =>
          resolve({
            evaluation_id: request.evaluation_id,
            ok: true,
            query_results: {
              query_1: {
                value: {
                  kind: "scalar",
                  value: { kind: "integer", value: 99 },
                },
                has_more: false,
              },
            },
            query_diagnostics: [],
            diagnostics: [],
          });
      })
    );
    renderStudio(draft);
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "query_1" }));
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());

    fireEvent.change(screen.getByRole("combobox", { name: "limit" }), {
      target: { value: "replacement" },
    });
    expect(screen.getByTestId("query-preview-status")).toHaveTextContent(
      "idle",
    );
    resolveEvaluation();
    await Promise.resolve();
    await Promise.resolve();

    expect(screen.getByTestId("query-preview-status")).toHaveTextContent(
      "idle",
    );
  });

  it("hides a query preview while a referenced literal is pending or invalid", async () => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [
      {
        id: "threshold",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 1 },
      },
      {
        id: "query_1",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT $limit",
          arguments: [{ name: "limit", variable: "threshold" }],
        },
      },
    ];
    renderStudio(draft);
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "query_1" }));
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() =>
      expect(screen.getByTestId("query-preview-status")).toHaveTextContent(
        "success",
      )
    );

    fireEvent.click(screen.getByRole("button", { name: "threshold" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Value" }), {
      target: { value: "bad" },
    });
    fireEvent.click(screen.getByRole("button", { name: "query_1" }));

    expect(screen.getByTestId("query-preview-status")).toHaveTextContent(
      "idle",
    );
  });

  it("runs the selected query through the graph evaluator", async () => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [{
      id: "query_1",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: { kind: "inline_sql", sql: "SELECT 1", arguments: [] },
    }];
    renderStudio(draft);
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "query_1" }));
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));

    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    expect(evaluateMock.mock.calls[0][1]).toMatchObject({
      target: { kind: "draft" },
      requested_queries: ["query_1"],
    });
    expect(evaluateMock.mock.calls[0][1]).not.toHaveProperty("query_results");
  });

  it("creates a Form-backed table query and component in one picker selection", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "Receipts",
      sql_relation: "form_receipts",
      fields: { amount: { type: "float", sql_column: "amount" } },
    }]);
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const picker = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Forms" }));
    fireEvent.click(
      await within(picker).findByRole("button", { name: "Receipts" }),
    );

    const block = await screen.findByRole("button", {
      name: "table, receipts_rows",
    });
    expect(block).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("button", { name: /Change variable/ }))
      .toBeNull();
    const authored = JSON.parse(previewGraphMock.mock.calls[0][1]);
    expect(authored.spec.variables[0].query).toMatchObject({
      kind: "inline_sql",
      sql: 'SELECT * FROM "form_receipts"',
      form_bindings: [],
      schema_fingerprint: "",
    });
    expect(authored.spec.components).toHaveLength(1);
    expect(authored.spec.components[0]).toMatchObject({
      kind: "table",
      variable: authored.spec.variables[0].id,
    });
    expect(authored.spec.layout.rows[0].items).toEqual([{
      kind: "component",
      component: authored.spec.components[0].id,
    }]);
  });

  it("uses readable Form labels and SQL columns for explicit aggregates", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "Receipts",
      sql_relation: "form_receipts",
      fields: {
        Amount: { type: "double", sql_column: "field_103" },
        Units: { type: "long", sql_column: "field_104" },
        Notes: { type: "markdown", sql_column: "field_105" },
        Query: { type: "sql", sql_column: "field_106" },
        Created: { type: "timestamp_tz_ns", sql_column: "field_107" },
      },
    }]);
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "scalar",
        value_type: { scalar_type: "float", nullable: true },
      })
    );
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Metric" }));
    const picker = screen.getByRole("dialog", { name: "Metric" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Forms" }));
    fireEvent.click(
      await within(picker).findByRole("button", { name: "Receipts" }),
    );
    fireEvent.change(
      within(picker).getByRole("combobox", { name: "Aggregation" }),
      { target: { value: "sum" } },
    );
    const column = within(picker).getByRole("combobox", { name: "Column" });
    expect(within(column).getByRole("option", { name: "Amount" }))
      .toHaveValue("field_103");
    expect(within(column).getByRole("option", { name: "Units" }))
      .toHaveValue("field_104");
    fireEvent.change(
      within(picker).getByRole("combobox", { name: "Aggregation" }),
      { target: { value: "max" } },
    );
    expect(within(column).getByRole("option", { name: "Notes" }))
      .toHaveValue("field_105");
    expect(within(column).getByRole("option", { name: "Query" }))
      .toHaveValue("field_106");
    expect(within(column).getByRole("option", { name: "Created" }))
      .toHaveValue("field_107");
    fireEvent.change(
      within(picker).getByRole("combobox", { name: "Aggregation" }),
      { target: { value: "sum" } },
    );
    fireEvent.change(column, { target: { value: "field_103" } });
    fireEvent.click(within(picker).getByRole("button", { name: "Add" }));

    expect(
      await screen.findByRole("button", {
        name: "metric, receipts_sum_amount",
      }),
    ).toBeVisible();
    expect(previewGraphMock).toHaveBeenCalledOnce();
    const authored = JSON.parse(previewGraphMock.mock.calls[0][1]);
    expect(authored.spec.variables[0].query.sql).toBe(
      'SELECT SUM("field_103") AS value FROM "form_receipts"',
    );
  });

  it("drops a Form preview resolved after the authenticated account changes", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "Receipts",
      sql_relation: "form_receipts",
      fields: { amount: { type: "float", sql_column: "amount" } },
    }]);
    let finishPreview!: (value: ReturnType<typeof plannedQuery>) => void;
    previewGraphMock.mockReturnValue(
      new Promise((resolve) => finishPreview = resolve),
    );
    authSessionMock.mockResolvedValue({
      authenticated: true,
      account: { account_id: "editor-1", display_name: "Editor" },
    });
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const picker = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Forms" }));
    fireEvent.click(
      await within(picker).findByRole("button", { name: "Receipts" }),
    );
    await waitFor(() => expect(previewGraphMock).toHaveBeenCalledOnce());

    authSessionMock.mockResolvedValue({
      authenticated: true,
      account: { account_id: "viewer-2", display_name: "Viewer" },
    });
    finishPreview(plannedQuery(previewGraphMock.mock.calls[0][1], {
      kind: "table",
      columns: [{
        name: "amount",
        value_type: { scalar_type: "float", nullable: false },
      }],
    }));

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.queryByRole("button", {
      name: "table, receipts_rows",
    })).toBeNull();
  });

  it("binds an exact scalar Saved SQL revision through the same picker", async () => {
    savedSqlMock.mockResolvedValue([{
      id: "sql-1",
      name: "Revenue total",
      kind: "user-query",
      revision_id: "sql-rev-2",
      variables: [],
    }]);
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "scalar",
        value_type: { scalar_type: "float", nullable: true },
      })
    );
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Metric" }));
    const picker = screen.getByRole("dialog", { name: "Metric" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Saved SQL" }));
    const savedSql = await within(picker).findByRole("button", {
      name: "Revenue total",
    });
    await waitFor(() => expect(savedSql).toBeEnabled());
    fireEvent.click(savedSql);

    expect(
      await screen.findByRole("button", {
        name: "metric, query_sql-1_value",
      }),
    ).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    const authored = JSON.parse(previewGraphMock.mock.calls[0][1]);
    expect(authored.spec.variables[0].query).toMatchObject({
      kind: "saved_sql",
      entry_id: "sql-1",
      revision_id: "sql-rev-2",
    });
    expect(authored.spec.components).toHaveLength(1);
    expect(authored.spec.components[0]).toMatchObject({
      kind: "metric",
      variable: authored.spec.variables[0].id,
    });
    expect(authored.spec.layout.rows[0].items).toEqual([{
      kind: "component",
      component: authored.spec.components[0].id,
    }]);
  });

  it("previews Saved SQL with a temporary component in an empty draft", async () => {
    savedSqlMock.mockResolvedValue([{
      id: "sql-table",
      name: "One column",
      kind: "user-query",
      revision_id: "sql-rev-1",
      variables: [],
    }]);
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const picker = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Saved SQL" }));
    const savedSql = await within(picker).findByRole("button", {
      name: "One column",
    });
    await waitFor(() => expect(savedSql).toBeEnabled());

    const preview = JSON.parse(previewGraphMock.mock.calls[0][1]);
    expect(preview.spec.components).toHaveLength(1);
    expect(preview.spec.components[0]).toMatchObject({
      kind: "table",
      variable: preview.spec.variables[0].id,
    });
    expect(preview.spec.layout.rows[0].items).toEqual([{
      kind: "component",
      component: preview.spec.components[0].id,
    }]);
    expect(screen.getByRole("button", { name: "One column" })).toBeEnabled();
    // Inspection is transient. It must not commit a probe component to the draft.
    expect(screen.queryByRole("button", { name: /query_sql-table/ }))
      .toBeNull();
  });

  it("keeps the preview probe transient when rebinding in an existing draft", async () => {
    savedSqlMock.mockResolvedValue([{
      id: "sql-table",
      name: "One column",
      kind: "user-query",
      revision_id: "sql-rev-1",
      variables: [],
    }]);
    const draft = renderedDraftWithTable();
    renderStudio(draft);
    fireEvent.click(screen.getByRole("button", { name: "table, rows" }));
    fireEvent.click(screen.getByRole("button", { name: /Change variable/ }));
    const picker = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Saved SQL" }));
    const savedSql = await within(picker).findByRole("button", {
      name: "One column",
    });
    await waitFor(() => expect(savedSql).toBeEnabled());

    const preview = JSON.parse(previewGraphMock.mock.calls[0][1]);
    expect(preview.spec.components).toHaveLength(2);
    expect(preview.spec.components).toEqual(expect.arrayContaining([
      { id: "table_1", kind: "table", variable: "rows" },
      expect.objectContaining({
        kind: "table",
        variable: "query_sql-table_table",
      }),
    ]));
    fireEvent.click(savedSql);

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("button", {
      name: "table, query_sql-table_table",
    })).toBeVisible();
    expect(screen.getAllByRole("button", { name: /^table,/ })).toHaveLength(1);
  });

  it("shows the incompatible Saved SQL result shape before Table selection", async () => {
    savedSqlMock.mockResolvedValue([{
      id: "sql-scalar",
      name: "One value",
      kind: "user-query",
      revision_id: "sql-rev-1",
      variables: [],
    }]);
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "scalar",
        value_type: { scalar_type: "float", nullable: false },
      })
    );
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Add to design" }));
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const picker = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Saved SQL" }));

    const reason = await within(picker).findByText(
      "This saved SQL revision does not return a table.",
    );
    expect(reason).toBeVisible();
    expect(within(picker).getByRole("button", { name: "One value" }))
      .toBeDisabled();
  });
});
