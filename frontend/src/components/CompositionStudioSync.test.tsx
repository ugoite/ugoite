import "@testing-library/jest-dom/vitest";
import type * as UgoiteClient from "~/lib/ugoite-client";
import type * as UgoiteClientProtocol from "~/lib/ugoite-client/protocol";
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
import { CompositionVariableGraphWorkspace } from "./CompositionVariableGraphWorkspace";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { buildSqlSchema } from "~/lib/sql";
import type { Form } from "~/lib/types";
import { setLocale } from "~/lib/i18n";

const {
  authSessionMock,
  navigateMock,
  formsMock,
  savedSqlMock,
  previewGraphMock,
  evaluateMock,
  countMock,
  canonicalizeMock,
  parseInputMock,
  saveMock,
  formReferenceGateMock,
  routePathMock,
} = vi.hoisted(
  () => ({
    authSessionMock: vi.fn(),
    navigateMock: vi.fn(),
    formsMock: vi.fn(),
    savedSqlMock: vi.fn(),
    previewGraphMock: vi.fn(),
    evaluateMock: vi.fn(),
    countMock: vi.fn(),
    canonicalizeMock: vi.fn(),
    parseInputMock: vi.fn(),
    saveMock: vi.fn(),
    formReferenceGateMock: vi.fn(),
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

vi.mock("~/lib/ugoite-client", async (importOriginal) => {
  const actual = await importOriginal<typeof UgoiteClient>();
  return {
    compositionFormQuerySql: actual.compositionFormQuerySql,
    parseSqlDocument: actual.parseSqlDocument,
    applySqlDocumentSemanticEditTransaction:
      actual.applySqlDocumentSemanticEditTransaction,
    renameSqlDocumentParameter: actual.renameSqlDocumentParameter,
    authApi: { getSession: (...args: unknown[]) => authSessionMock(...args) },
    formApi: { list: (...args: unknown[]) => formsMock(...args) },
    sqlApi: {
      list: (...args: unknown[]) => savedSqlMock(...args),
      count: (...args: unknown[]) => countMock(...args),
    },
    parseVariableGraphInput: (...args: unknown[]) => parseInputMock(...args),
  };
});

vi.mock("~/lib/ugoite-client/protocol", async (importOriginal) => {
  const actual = await importOriginal<typeof UgoiteClientProtocol>();
  return {
    ...actual,
    collectSqlFormRelationNames: async (sql: string) => {
      const references = await actual.collectSqlFormRelationNames(sql);
      return await formReferenceGateMock(references);
    },
  };
});

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
    readOnly?: boolean;
    actions?: Array<
      { label: string; disabled?: boolean; onActivate: () => void }
    >;
    onCount?: () => void;
    onPrevious?: () => void;
    onNext?: () => void;
    onSqlChange?: (value: string) => void;
    onPreview?: () => void;
    previewDisabled?: boolean;
    preview?: {
      status: string;
      isCurrent: boolean;
      page?: { pageNumber?: number };
      count?: { status: string; value?: number };
    };
  }) => (
    <>
      <textarea
        aria-label="SQL"
        value={props.sql}
        readOnly={props.readOnly}
        onInput={(event) => props.onSqlChange?.(event.currentTarget.value)}
      />
      <button
        type="button"
        disabled={props.previewDisabled}
        onClick={() => props.onPreview?.()}
      >
        Run query
      </button>
      {props.actions?.map((action) => (
        <button
          type="button"
          disabled={action.disabled}
          onClick={() => action.onActivate()}
        >
          {action.label}
        </button>
      ))}
      <button type="button" onClick={props.onCount}>Count query</button>
      <button type="button" onClick={props.onPrevious}>
        Previous query page
      </button>
      <button type="button" onClick={props.onNext}>Next query page</button>
      <span data-testid="query-preview-status">{props.preview?.status}</span>
      <span data-testid="query-page">{props.preview?.page?.pageNumber}</span>
      <span data-testid="query-count">
        {props.preview?.count?.value ?? props.preview?.count?.status ?? ""}
      </span>
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

let releasePendingFormReferences: ((references: string[]) => void) | undefined;

describe("CompositionStudio graph surface", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    formReferenceGateMock.mockImplementation(async (references: string[]) =>
      references
    );
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
    countMock.mockReset().mockResolvedValue(102);
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
      request: { evaluation_id: string; requested_queries?: string[] },
    ) =>
      Promise.resolve({
        evaluation_id: request.evaluation_id,
        ok: true,
        query_results: Object.fromEntries(
          (request.requested_queries ?? ["query_1"]).map((queryId) => [
            queryId,
            {
              value: {
                kind: "scalar",
                value: { kind: "integer", value: 1 },
              },
              has_more: false,
            },
          ]),
        ),
        query_diagnostics: [],
        diagnostics: [],
      })
    );
  });
  afterEach(() => {
    releasePendingFormReferences?.(["Receipts"]);
    releasePendingFormReferences = undefined;
    cleanup();
  });

  it("displays the authorized pinned Saved SQL source without reading the moving head", async () => {
    const initial = renderedDraftWithTable();
    initial.variables[0] = {
      id: "rows",
      kind: "query",
      returns: { kind: "table", columns: [] },
      query: {
        kind: "saved_sql",
        entry_id: "saved-1",
        revision_id: "revision-pinned",
        arguments: [],
      },
    };
    previewGraphMock.mockResolvedValue({
      ok: true,
      plan: {
        queries: [{
          variable_id: "rows",
          sql_text: "SELECT 11 AS pinned_value",
          portable_sql: "SELECT 11 AS pinned_value",
          deferred_arguments: [],
          returns: { kind: "table", columns: [] },
          result_columns: [],
          form_bindings: [],
          schema_fingerprint: "pinned-schema",
          query_semantic_fingerprint: "pinned-meaning",
        }],
      },
    });
    savedSqlMock.mockResolvedValue([{
      id: "saved-1",
      name: "Latest",
      revision_id: "revision-latest",
      kind: "user-query",
      sql: "SELECT 99 AS latest_value",
      variables: [],
    }]);
    renderStudio(initial);
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
        "SELECT 11 AS pinned_value",
      )
    );
    const requested = JSON.parse(previewGraphMock.mock.calls[0][1]);
    expect(requested.spec.variables[0].query).toMatchObject({
      entry_id: "saved-1",
      revision_id: "revision-pinned",
    });
    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveAttribute(
      "readonly",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Edit an inline copy" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "SQL" })).not.toHaveAttribute(
        "readonly",
      )
    );
    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
      "SELECT 11 AS pinned_value",
    );
    expect(saveMock).not.toHaveBeenCalled();
    expect(evaluateMock).not.toHaveBeenCalled();
  });

  it("counts only on request, pages in both directions, and rejects an obsolete count", async () => {
    evaluateMock.mockImplementation((
      _space: string,
      request: { evaluation_id: string; page?: unknown },
    ) => ({
      evaluation_id: request.evaluation_id,
      ok: true,
      variable_values: {},
      queries: [{
        variable_id: "rows",
        portable_sql: "SELECT field_100 FROM form_immutable",
        returns: { kind: "table", columns: [] },
      }],
      query_results: {
        rows: {
          value: {
            kind: "table",
            columns: [{
              name: "amount",
              value_type: { scalar_type: "integer", nullable: false },
            }],
            rows: [[{ kind: "integer", value: request.page ? 2 : 1 }]],
          },
          has_more: !request.page,
          next: request.page ? undefined : "next-page",
        },
      },
    }));
    renderStudio(renderedDraftWithTable());
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() =>
      expect(screen.getByTestId("query-preview-status")).toHaveTextContent(
        "success",
      )
    );
    expect(countMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Count query" }));
    await waitFor(() =>
      expect(screen.getByTestId("query-count")).toHaveTextContent("102")
    );
    expect(countMock.mock.calls[0][1]).toEqual({
      sql: "SELECT field_100 FROM form_immutable",
      parameters: {},
      parameter_types: {},
    });
    fireEvent.click(screen.getByRole("button", { name: "Next query page" }));
    await waitFor(() =>
      expect(screen.getByTestId("query-page")).toHaveTextContent("2")
    );
    expect(evaluateMock.mock.calls[1][1].page).toEqual({
      variable_id: "rows",
      cursor: "next-page",
    });
    // Paging one Workspace query must not execute unrelated graph branches.
    expect(evaluateMock.mock.calls[1][1].requested_queries).toEqual(["rows"]);
    fireEvent.click(
      screen.getByRole("button", { name: "Previous query page" }),
    );
    expect(screen.getByTestId("query-page")).toHaveTextContent("1");
    expect(evaluateMock).toHaveBeenCalledTimes(2);
    let completeCount!: (value: number) => void;
    countMock.mockImplementationOnce(() =>
      new Promise<number>((resolve) => {
        completeCount = resolve;
      })
    );
    fireEvent.click(screen.getByRole("button", { name: "Count query" }));
    await waitFor(() => expect(countMock).toHaveBeenCalledTimes(2));
    fireEvent.input(screen.getByRole("textbox", { name: "SQL" }), {
      target: { value: "SELECT 2" },
    });
    completeCount(999);
    await Promise.resolve();
    expect(screen.getByTestId("query-preview-status")).toHaveTextContent(
      "idle",
    );
    expect(screen.getByTestId("query-count")).not.toHaveTextContent("999");
    expect(countMock.mock.calls[1][2].aborted).toBe(true);
  });

  it("clears draft and preview Work when a mounted create route changes Space", async () => {
    const [spaceId, setSpaceId] = createSignal("space-1");
    let finish!: (value: unknown) => void;
    evaluateMock.mockImplementation((
      _space: string,
      request: { evaluation_id: string },
    ) =>
      new Promise((resolve) => {
        finish = (value) =>
          resolve({ evaluation_id: request.evaluation_id, ...value as object });
      })
    );
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        initialDraft={renderedDraftWithTable()}
        saveMode={{ kind: "create" }}
        backHref="/spaces"
        backLabel="Back"
      />
    ));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    setSpaceId("space-2");
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("");
    expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(evaluateMock.mock.calls[0][2].aborted).toBe(true);
    finish({
      ok: true,
      query_results: {
        rows: {
          value: { kind: "table", columns: [], rows: [] },
          has_more: false,
        },
      },
    });
    await Promise.resolve();
    fireEvent.click(screen.getByRole("tab", { name: "Variables" }));
    expect(screen.queryByRole("button", { name: "rows" })).toBeNull();
    expect(screen.queryByTestId("query-preview-status")).toBeNull();
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("offers only Design and Variables and inserts a Table without opening an Inspector", async () => {
    renderStudio();
    const designTab = await screen.findByRole("tab", { name: "Design" });
    const variablesTab = screen.getByRole("tab", { name: "Variables" });
    expect(designTab).toBeVisible();
    expect(variablesTab).toBeVisible();
    expect(designTab.closest('[role="tablist"]')).toHaveClass("studioMode");
    expect(designTab).toHaveClass(
      "studioModeOption",
      "studioModeOption--active",
    );
    expect(variablesTab).toHaveClass("studioModeOption");
    expect(screen.queryByRole("tab", { name: "Data" })).toBeNull();

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
    fireEvent.click(screen.getByRole("button", { name: "Input" }));

    expect(await screen.findByRole("button", { name: "input, input_1" }))
      .toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.click(screen.getByRole("button", { name: "input, input_1" }));
    fireEvent.click(
      screen.getByRole("button", { name: "input_1", exact: true }),
    );
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Variable ID" }))
        .toHaveFocus()
    );
    fireEvent.click(screen.getByRole("tab", { name: "Design" }));
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
      { variables: {}, unset_variables: [] },
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
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
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

  it("discards an in-flight query preview when a referenced input default changes", async () => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [
      {
        id: "minimum_amount",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
        default: { kind: "integer", value: 1 },
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
          arguments: [{ name: "limit", variable: "minimum_amount" }],
        },
      },
    ];
    let resolveEvaluation!: () => void;
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
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());

    fireEvent.click(screen.getByRole("button", { name: "minimum_amount" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Default" }), {
      target: { value: "2" },
    });
    resolveEvaluation();
    await Promise.resolve();
    await Promise.resolve();

    fireEvent.click(screen.getByRole("button", { name: "query_1" }));
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
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
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
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));

    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    expect(evaluateMock.mock.calls[0][1]).toMatchObject({
      target: { kind: "draft" },
      requested_queries: ["query_1"],
    });
    expect(evaluateMock.mock.calls[0][1]).not.toHaveProperty("query_results");
  });

  it("writes the real SQL toolbar Form selection into the host before first preview and clears pending per query", async () => {
    const pendingReferenceResolvers: Array<(references: string[]) => void> = [];
    formReferenceGateMock.mockImplementation((references: string[]) =>
      references.length === 0 ? Promise.resolve([]) : new Promise((resolve) => {
        pendingReferenceResolvers.push(resolve);
        releasePendingFormReferences = resolve;
      })
    );
    const forms: Form[] = [
      {
        id: "form-id",
        name: "Receipts",
        sql_relation: "form_receipts",
        version: 1,
        template: "",
        fields: {
          amount: { type: "float", required: false, sql_column: "amount" },
        },
      },
      {
        id: "form-id-b",
        name: "Expenses",
        sql_relation: "form_expenses",
        version: 1,
        template: "",
        fields: {
          amount: { type: "float", required: false, sql_column: "amount" },
        },
      },
    ];
    const draft = draftWithTable();
    draft.variables[0] = {
      ...draft.variables[0],
      query: {
        kind: "inline_sql",
        sql: "SELECT 1",
        arguments: [],
        form_bindings: [],
        schema_fingerprint: "",
      },
    };
    draft.variables.push({
      id: "other",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: { kind: "inline_sql", sql: "SELECT 2", arguments: [] },
    });
    const [hostDraft, setHostDraft] = createSignal(draft);
    const [selectedVariableId, setSelectedVariableId] = createSignal<
      string | null
    >("rows");
    const runDrafts: CompositionGraphDraft[] = [];
    render(() => (
      <>
        <CompositionVariableGraphWorkspace
          draft={hostDraft()}
          selectedVariableId={selectedVariableId()}
          onSelect={setSelectedVariableId}
          onDraftChange={setHostDraft}
          onRemove={(id) =>
            setHostDraft((current) => ({
              ...current,
              variables: current.variables.filter((node) => node.id !== id),
            }))}
          sqlSchema={buildSqlSchema([])}
          sqlForms={forms}
          onQueryChange={(id, sql, bindings) =>
            setHostDraft((current) => ({
              ...current,
              variables: current.variables.map((node) =>
                node.id === id && node.kind === "query" &&
                  node.query.kind === "inline_sql"
                  ? {
                    ...node,
                    query: {
                      ...node.query,
                      sql,
                      form_bindings: bindings ?? [],
                    },
                  }
                  : node
              ),
            }))}
          onRunQuery={() => runDrafts.push(hostDraft())}
          onCountQuery={() => {}}
        />
        <output data-testid="host-draft">{JSON.stringify(hostDraft())}</output>
      </>
    ));
    const formAction = await screen.findByRole("button", { name: "Form" });
    await waitFor(() => expect(formAction).toBeEnabled());
    fireEvent.click(formAction);
    const picker = screen.getByRole("dialog");
    const receipts = within(picker).getByRole("button", { name: "Receipts" });
    await waitFor(() => expect(receipts).toBeEnabled());
    fireEvent.click(receipts);

    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
        'SELECT 1 FROM "Receipts"',
      )
    );
    expect(
      JSON.parse(screen.getByTestId("host-draft").textContent!).variables
        .find((node: { id: string }) => node.id === "rows").query,
    ).toMatchObject({
      sql: 'SELECT 1 FROM "Receipts"',
      form_bindings: [{ relation: "Receipts", form_id: "form-id" }],
    });
    await waitFor(() => expect(pendingReferenceResolvers).toHaveLength(1));
    expect(screen.getByRole("button", { name: "Run query" })).toBeDisabled();
    expect(runDrafts).toHaveLength(0);

    // Completion while another query is selected must clear only the rows
    // query's pending state; switching back must not leave its actions stuck.
    fireEvent.click(screen.getByRole("button", { name: "other" }));
    pendingReferenceResolvers[0](["Receipts"]);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    expect(runDrafts).toHaveLength(1);
    expect(runDrafts[0].variables.find((node) => node.id === "rows"))
      .toMatchObject({
        query: {
          sql: 'SELECT 1 FROM "Receipts"',
          form_bindings: [{ relation: "Receipts", form_id: "form-id" }],
        },
      });

    fireEvent.click(screen.getByRole("button", { name: "Form" }));
    const expenses = within(screen.getByRole("dialog")).getByRole("button", {
      name: "Expenses",
    });
    await waitFor(() => expect(expenses).toBeEnabled());
    fireEvent.click(expenses);
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
        'SELECT 1 FROM "Expenses"',
      )
    );
    expect(
      JSON.parse(screen.getByTestId("host-draft").textContent!).variables
        .find((node: { id: string }) => node.id === "rows").query,
    ).toMatchObject({
      sql: 'SELECT 1 FROM "Expenses"',
      form_bindings: [
        { relation: "Receipts", form_id: "form-id" },
        { relation: "Expenses", form_id: "form-id-b" },
      ],
    });
    await waitFor(() => expect(pendingReferenceResolvers).toHaveLength(2));
    fireEvent.click(screen.getByRole("button", { name: "other" }));
    pendingReferenceResolvers[1](["Expenses"]);
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled()
    );
  });

  it("creates a Form-backed table query and component in one picker selection", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "Receipts",
      sql_relation: "form_receipts",
      fields: { amount: { type: "float", sql_column: "amount" } },
    }]);
    renderStudio(createCompositionGraphDraft("Report"));
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
      form_bindings: [{ relation: "form_receipts", form_id: "form-id" }],
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

  it("inserts existing Form data before the Composition is named without enabling Save", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "Receipts",
      sql_relation: "form_receipts",
      fields: { Amount: { type: "float", sql_column: "amount" } },
    }]);
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) => {
      if (!JSON.parse(yaml).name.trim()) {
        return { ok: false, diagnostics: [{ code: "invalid_composition" }] };
      }
      return plannedQuery(yaml, {
        kind: "table",
        columns: [{
          name: "amount",
          value_type: { scalar_type: "float", nullable: false },
        }],
      });
    });
    renderStudio(createCompositionGraphDraft(""));
    fireEvent.click(screen.getByRole("button", { name: "Table", exact: true }));
    const picker = screen.getByRole("dialog", { name: "Table", exact: true });
    fireEvent.click(
      within(picker).getByRole("tab", { name: "Forms", exact: true }),
    );
    fireEvent.click(
      await within(picker).findByRole("button", {
        name: "Receipts",
        exact: true,
      }),
    );
    expect(
      await screen.findByRole("button", {
        name: "table, receipts_rows",
        exact: true,
      }),
    ).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("textbox", { name: "Name", exact: true }))
      .toHaveValue("");
    expect(screen.getByRole("button", { name: "Save", exact: true }))
      .toBeDisabled();
    expect(saveMock).not.toHaveBeenCalled();
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
    expect(authored.spec.variables[0].query.form_bindings).toEqual([{
      relation: "form_receipts",
      form_id: "form-id",
    }]);
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
        name: "metric, revenue_total_value",
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
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "table",
        columns: [{
          name: "amount",
          value_type: { scalar_type: "float", nullable: true },
        }],
      })
    );
    savedSqlMock.mockResolvedValue([{
      id: "sql-table",
      name: "One column",
      kind: "user-query",
      revision_id: "sql-rev-1",
      variables: [],
    }]);
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const picker = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Saved SQL" }));
    const savedSql = await within(picker).findByRole("button", {
      name: "One column",
    });
    await waitFor(() => expect(savedSql).toBeEnabled());

    expect(previewGraphMock).not.toHaveBeenCalled();
    fireEvent.click(savedSql);
    await waitFor(() => expect(previewGraphMock).toHaveBeenCalled());
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
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getAllByRole("button", { name: /^table,/ })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "table, one_column_table" }))
      .toBeVisible();
  });

  it("keeps the preview probe transient when rebinding in an existing draft", async () => {
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "table",
        columns: [{
          name: "amount",
          value_type: { scalar_type: "float", nullable: true },
        }],
      })
    );
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

    expect(previewGraphMock).not.toHaveBeenCalled();
    fireEvent.click(savedSql);
    await waitFor(() => expect(previewGraphMock).toHaveBeenCalled());
    const preview = JSON.parse(previewGraphMock.mock.calls[0][1]);
    expect(preview.spec.components).toHaveLength(2);
    expect(preview.spec.components).toEqual(expect.arrayContaining([
      { id: "table_1", kind: "table", variable: "rows" },
      expect.objectContaining({
        kind: "table",
        variable: "one_column_table",
      }),
    ]));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("button", {
      name: "table, one_column_table",
    })).toBeVisible();
    expect(screen.getAllByRole("button", { name: /^table,/ })).toHaveLength(1);
  });

  it("reports the selected incompatible Saved SQL without inserting a Table", async () => {
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
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const picker = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(picker).getByRole("tab", { name: "Saved SQL" }));

    const selected = await within(picker).findByRole("button", {
      name: "One value",
    });
    expect(selected).toBeEnabled();
    expect(previewGraphMock).not.toHaveBeenCalled();
    fireEvent.click(selected);
    const reason = await within(picker).findByText(
      "This saved SQL revision does not return a table.",
    );
    expect(reason).toBeVisible();
    expect(screen.queryByRole("button", { name: /^table,/ })).not
      .toBeInTheDocument();
  });
});
