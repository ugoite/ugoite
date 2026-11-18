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
import { clearPendingCompositionSaveAttempt } from "~/lib/composition-save-attempt";
import { buildSqlSchema } from "~/lib/sql";
import type { Form, FormField } from "~/lib/types";
import { setLocale } from "~/lib/i18n";

const {
  authSessionMock,
  navigateMock,
  formsMock,
  previewGraphMock,
  evaluateMock,
  countMock,
  canonicalizeMock,
  parseInputMock,
  saveMock,
  compositionFormQuerySqlMock,
  formReferenceGateMock,
  routePathMock,
} = vi.hoisted(
  () => ({
    authSessionMock: vi.fn(),
    navigateMock: vi.fn(),
    formsMock: vi.fn(),
    previewGraphMock: vi.fn(),
    evaluateMock: vi.fn(),
    countMock: vi.fn(),
    canonicalizeMock: vi.fn(),
    parseInputMock: vi.fn(),
    saveMock: vi.fn(),
    compositionFormQuerySqlMock: vi.fn(),
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
  compositionFormQuerySqlMock.mockImplementation(
    actual.compositionFormQuerySql,
  );
  return {
    compositionFormQuerySql: (
      ...args: Parameters<typeof actual.compositionFormQuerySql>
    ) => compositionFormQuerySqlMock(...args),
    parseSqlDocument: actual.parseSqlDocument,
    applySqlDocumentSemanticEditTransaction:
      actual.applySqlDocumentSemanticEditTransaction,
    renameSqlDocumentParameter: actual.renameSqlDocumentParameter,
    authApi: { getSession: (...args: unknown[]) => authSessionMock(...args) },
    formApi: { list: (...args: unknown[]) => formsMock(...args) },
    sqlApi: {
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
    onCheck?: () => void;
    planner?: {
      status: "pending" | "success" | "error";
      isCurrent: boolean;
      error?: string;
    };
    previewDisabled?: boolean;
    hideRun?: boolean;
    onAccept?: () => void;
    acceptLabel?: string;
    acceptDisabled?: boolean;
    onCancel?: () => void;
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
      {!props.hideRun && (
        <button
          type="button"
          disabled={props.previewDisabled || Boolean(
            props.onCheck &&
              (props.planner?.status !== "success" ||
                !props.planner.isCurrent),
          )}
          onClick={() => props.onPreview?.()}
        >
          Run query
        </button>
      )}
      {props.onAccept && (
        <button
          type="button"
          disabled={props.acceptDisabled}
          onClick={() => props.onAccept?.()}
        >
          {props.acceptLabel}
        </button>
      )}
      <span data-testid="query-preview-disabled">
        {props.previewDisabled ? "true" : "false"}
      </span>
      {props.onCheck && (
        <button
          type="button"
          disabled={props.planner?.status === "pending" &&
            props.planner.isCurrent}
          onClick={() => props.onCheck?.()}
        >
          Check
        </button>
      )}
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
      <span data-testid="query-plan-status">{props.planner?.status}</span>
      <span data-testid="query-plan-error">{props.planner?.error}</span>
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
      sql: "SELECT 1 AS amount",
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
        form_bindings: query.query?.form_bindings ?? [],
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

const createTableQueryFromDesign = async (id: string) => {
  fireEvent.click(screen.getByRole("button", { name: "Table", exact: true }));
  const name = screen.getByRole("textbox", { name: "Variable name" });
  fireEvent.input(name, { target: { value: id } });
  fireEvent.submit(name.closest("form")!);
  return await screen.findByRole("textbox", { name: "SQL" });
};

const selectFormForQuery = async (name: string) => {
  fireEvent.click(screen.getByRole("button", { name: "Form", exact: true }));
  const form = await screen.findByRole("button", { name, exact: true });
  await waitFor(() => expect(form).toBeEnabled());
  fireEvent.click(form);
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Form", exact: true }))
      .toBeEnabled()
  );
  await waitFor(() =>
    expect(screen.getByTestId("query-preview-disabled"))
      .toHaveTextContent("true")
  );
  await waitFor(
    () =>
      expect(screen.getByTestId("query-preview-disabled"))
        .toHaveTextContent("false"),
    { timeout: 10_000 },
  );
  await checkQuery();
};

const checkQuery = async () => {
  await waitFor(
    () =>
      expect(screen.getByTestId("query-preview-disabled"))
        .toHaveTextContent("false"),
    { timeout: 10_000 },
  );
  fireEvent.click(await screen.findByRole("button", { name: "Check" }));
  await waitFor(() =>
    expect(["success", "error"]).toContain(
      screen.getByTestId("query-plan-status").textContent,
    )
  );
  expect(screen.getByTestId("query-plan-status")).toHaveTextContent("success");
  const useButton = screen.queryByRole("button", { name: "Use", exact: true });
  if (useButton) {
    expect(screen.queryByRole("button", { name: "Run query" }))
      .not.toBeInTheDocument();
    expect(useButton).toBeEnabled();
  } else {
    expect(screen.getByRole("button", { name: "Run query" })).toBeEnabled();
  }
};

let releasePendingFormReferences: ((references: string[]) => void) | undefined;

const clearPendingSaveAttempts = () => {
  clearPendingCompositionSaveAttempt({
    spaceId: "space-1",
    principalId: "account:editor-1",
    routePath: "/spaces/space-1/compositions/new",
    saveTarget: { kind: "create" },
  });
  clearPendingCompositionSaveAttempt({
    spaceId: "space-1",
    principalId: "account:editor-1",
    routePath: "/spaces/space-1/compositions/composition-2/edit",
    saveTarget: {
      kind: "update",
      compositionId: "composition-2",
      baseRevisionId: "revision-2",
    },
  });
};

const projectableField = (
  field: FormField,
  name: string,
  fieldId: number,
): FormField => ({
  ...field,
  query_capability: {
    field: { kind: "property", field_id: fieldId },
    name,
    field_type: field.type,
    filterable: true,
    sortable: true,
    projectable: true,
    supported_operators: ["equals"],
  },
});

describe("CompositionStudio graph surface", () => {
  beforeEach(() => {
    setLocale("en");
    for (
      const mock of [
        authSessionMock,
        navigateMock,
        formsMock,
        previewGraphMock,
        evaluateMock,
        countMock,
        canonicalizeMock,
        parseInputMock,
        saveMock,
        formReferenceGateMock,
      ]
    ) mock.mockReset();
    routePathMock.value = "/spaces/space-1/compositions/new";
    clearPendingSaveAttempts();
    formReferenceGateMock.mockImplementation(async (references: string[]) =>
      references
    );
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
    compositionFormQuerySqlMock.mockClear();
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
                value: { kind: "integer", value: "1" },
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
    routePathMock.value = "/spaces/space-1/compositions/new";
    clearPendingSaveAttempts();
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
          count_snapshot: "count-snapshot-token",
        },
      },
    }));
    renderStudio(renderedDraftWithTable());
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await checkQuery();
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
      composition_count_snapshot: "count-snapshot-token",
      parameters: {},
      parameter_types: {},
    });
    fireEvent.click(screen.getByRole("button", { name: "Next query page" }));
    await waitFor(() =>
      expect(screen.getByTestId("query-page")).toHaveTextContent("2")
    );
    const nextPageRequest = evaluateMock.mock.calls.find(([, request]) =>
      request.page?.cursor === "next-page"
    );
    expect(nextPageRequest?.[1].page).toEqual({
      variable_id: "rows",
      cursor: "next-page",
    });
    // Paging one Workspace query must not execute unrelated graph branches.
    expect(nextPageRequest?.[1].requested_queries).toEqual(["rows"]);
    fireEvent.click(
      screen.getByRole("button", { name: "Previous query page" }),
    );
    expect(screen.getByTestId("query-page")).toHaveTextContent("1");
    // Design preview can evaluate independently; returning to a cached page
    // must not issue another cursor-based request.
    expect(
      evaluateMock.mock.calls.filter(([, request]) => request.page),
    ).toHaveLength(1);
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
    await checkQuery();
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    setSpaceId("space-2");
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("");
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

  it("drops a mounted pending Table creation when Space changes", async () => {
    const [spaceId, setSpaceId] = createSignal("space-1");
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        saveMode={{ kind: "create" }}
        backHref="/spaces"
        backLabel="Back"
      />
    ));

    const sql = await createTableQueryFromDesign("space_a_rows");
    expect(sql).toHaveValue("SELECT 1");
    setSpaceId("space-2");

    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("");
      expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });
    expect(screen.queryByRole("button", { name: "Use", exact: true }))
      .not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Variables" }));
    expect(screen.queryByRole("button", { name: "space_a_rows" }))
      .not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Design" }));
    const nextSql = await createTableQueryFromDesign("space_b_rows");
    expect(nextSql).toHaveValue("SELECT 1");
  });

  it("drops nested marker Input Work before and after Use when Space changes", async () => {
    const [spaceId, setSpaceId] = createSignal("space-1");
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        saveMode={{ kind: "create" }}
        backHref="/spaces"
        backLabel="Back"
      />
    ));

    const createMarkerInput = async (queryId: string) => {
      const sql = await createTableQueryFromDesign(queryId);
      fireEvent.input(sql, {
        target: { value: "SELECT {{month}} AS value" },
      });
      fireEvent.click(
        await screen.findByRole("button", {
          name: "Create a variable for month",
        }),
      );
      fireEvent.click(await screen.findByRole("button", { name: "Input" }));
      const name = await screen.findByRole("textbox", {
        name: "Variable name",
      });
      fireEvent.submit(name.closest("form")!);
      await screen.findByRole("combobox", { name: "Type" });
    };

    await createMarkerInput("space_a_rows");
    setSpaceId("space-2");
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("");
      expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });
    expect(screen.queryByRole("button", { name: "Use", exact: true }))
      .not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Variables" }));
    expect(screen.queryByRole("button", { name: "space_a_rows" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "month" }))
      .not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Design" }));
    await createMarkerInput("space_b_rows");
    const inputUse = (await screen.findAllByRole("button", {
      name: "Use",
      exact: true,
    })).find((button) => !(button as HTMLButtonElement).disabled);
    expect(inputUse).toBeDefined();
    fireEvent.click(inputUse!);
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "SQL" })).toHaveValue(
        "SELECT {{month}} AS value",
      );
    });
    setSpaceId("space-3");
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("");
      expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });
    fireEvent.click(screen.getByRole("tab", { name: "Variables" }));
    expect(screen.queryByRole("button", { name: "space_b_rows" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "month" }))
      .not.toBeInTheDocument();
  });

  it("discards a delayed Check after Space changes and the query ID is reused", async () => {
    const [spaceId, setSpaceId] = createSignal("space-1");
    let resolveOldCheck!: (value: unknown) => void;
    previewGraphMock.mockImplementationOnce((_space: string, _yaml: string) =>
      new Promise((resolve) => {
        resolveOldCheck = (value) => resolve(value);
      })
    );
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        saveMode={{ kind: "create" }}
        backHref="/spaces"
        backLabel="Back"
      />
    ));

    const oldSql = await createTableQueryFromDesign("rows");
    fireEvent.input(oldSql, { target: { value: "SELECT 4 AS old_value" } });
    fireEvent.click(screen.getByRole("button", { name: "Check" }));
    await waitFor(() => expect(previewGraphMock).toHaveBeenCalledOnce());
    const oldYaml = previewGraphMock.mock.calls[0][1];

    setSpaceId("space-2");
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Name" }))
        .toHaveValue("")
    );
    const newSql = await createTableQueryFromDesign("rows");
    fireEvent.input(newSql, { target: { value: "SELECT 4 AS old_value" } });
    await checkQuery();
    expect(screen.getByRole("button", { name: "Use", exact: true }))
      .toBeEnabled();

    resolveOldCheck(plannedQuery(oldYaml, {
      kind: "table",
      columns: [{
        name: "old_value",
        value_type: { scalar_type: "integer", nullable: false },
      }],
    }));
    await Promise.resolve();
    expect(screen.getByRole("textbox", { name: "SQL" }))
      .toHaveValue("SELECT 4 AS old_value");
    expect(screen.getByRole("button", { name: "Use", exact: true }))
      .toBeEnabled();
  });

  it("discards a delayed literal parse after Space changes and the variable ID is reused", async () => {
    const [spaceId, setSpaceId] = createSignal("space-1");
    const initialDraft = createCompositionGraphDraft("Report");
    initialDraft.variables = [{
      id: "month",
      kind: "constant",
      value_type: { scalar_type: "string", nullable: false },
      value: { kind: "string", value: "old" },
    }];
    let resolveOldParse!: (value: unknown) => void;
    parseInputMock.mockImplementationOnce(() =>
      new Promise((resolve) => {
        resolveOldParse = (value) => resolve(value);
      })
    );
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        initialDraft={initialDraft}
        saveMode={{ kind: "create" }}
        backHref="/spaces"
        backLabel="Back"
      />
    ));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "month" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Value" }), {
      target: { value: "old-scope-value" },
    });
    await waitFor(() => expect(parseInputMock).toHaveBeenCalledOnce());

    setSpaceId("space-2");
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Name" }))
        .toHaveValue("")
    );
    fireEvent.click(screen.getByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(
      screen.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "Value", exact: true }),
    );
    const name = await screen.findByRole("textbox", { name: "Variable name" });
    fireEvent.input(name, { target: { value: "month" } });
    fireEvent.submit(name.closest("form")!);
    const currentValue = await screen.findByRole("textbox", { name: "Value" });
    expect(currentValue).toHaveValue("");

    resolveOldParse({ status: "valid", value: "old-scope-value" });
    await Promise.resolve();
    expect(currentValue).toHaveValue("");
  });

  it("discards delayed Form-guard parsing after Space changes and a query ID is reused", async () => {
    const [spaceId, setSpaceId] = createSignal("space-1");
    let resolveOldGuard!: (references: string[]) => void;
    formReferenceGateMock.mockImplementationOnce(() =>
      new Promise((resolve) => {
        resolveOldGuard = resolve;
      })
    );
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        initialDraft={draftWithTable()}
        saveMode={{ kind: "create" }}
        backHref="/spaces"
        backLabel="Back"
      />
    ));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    fireEvent.input(screen.getByRole("textbox", { name: "SQL" }), {
      target: { value: 'SELECT 1 FROM "Receipts"' },
    });
    await waitFor(() => expect(formReferenceGateMock).toHaveBeenCalledOnce());

    setSpaceId("space-2");
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Name" }))
        .toHaveValue("")
    );
    fireEvent.click(screen.getByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(
      screen.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "SQL", exact: true }),
    );
    const name = await screen.findByRole("textbox", { name: "Variable name" });
    fireEvent.input(name, { target: { value: "rows" } });
    fireEvent.submit(name.closest("form")!);
    const currentSql = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => expect(currentSql).toHaveValue("SELECT 1"));
    fireEvent.input(currentSql, {
      target: { value: 'SELECT 1 FROM "Receipts"' },
    });
    await waitFor(() =>
      expect(currentSql).toHaveValue(
        'SELECT 1 FROM "Receipts"',
      )
    );

    resolveOldGuard(["Receipts"]);
    await Promise.resolve();
    expect(currentSql).toHaveValue('SELECT 1 FROM "Receipts"');
  });

  it("clears a manual SQL marker unbind when the editor scope changes", async () => {
    const [spaceId, setSpaceId] = createSignal("space-1");
    const initialDraft = createCompositionGraphDraft("Report");
    initialDraft.variables = [
      {
        id: "month",
        kind: "constant",
        value_type: { scalar_type: "string", nullable: false },
        value: { kind: "string", value: "January" },
      },
      {
        id: "rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "inline_sql",
          sql: "SELECT {{month}} AS value",
          arguments: [{ name: "month", variable: "month" }],
        },
      },
    ];
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        initialDraft={initialDraft}
        saveMode={{ kind: "create" }}
        backHref="/spaces"
        backLabel="Back"
      />
    ));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    const binding = await screen.findByRole("combobox", {
      name: "Choose a variable for month",
    });
    fireEvent.change(binding, { target: { value: "" } });
    await waitFor(() => expect(binding).toHaveValue(""));

    setSpaceId("space-2");
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Name" }))
        .toHaveValue("")
    );
    fireEvent.click(screen.getByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(
      screen.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "Value", exact: true }),
    );
    const constantName = await screen.findByRole("textbox", {
      name: "Variable name",
    });
    fireEvent.input(constantName, { target: { value: "month" } });
    fireEvent.submit(constantName.closest("form")!);

    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(
      screen.getByRole("group", { name: "Add variable" })
        .getByRole("button", { name: "SQL", exact: true }),
    );
    const queryName = await screen.findByRole("textbox", {
      name: "Variable name",
    });
    fireEvent.input(queryName, { target: { value: "rows" } });
    fireEvent.submit(queryName.closest("form")!);
    const sql = await screen.findByRole("textbox", { name: "SQL" });
    fireEvent.input(sql, { target: { value: "SELECT {{month}} AS value" } });
    await waitFor(() =>
      expect(screen.getByRole("combobox", {
        name: "Choose a variable for month",
      })).toHaveValue("month")
    );
  });

  it("resets same-Space editing when the target revision or principal changes", async () => {
    const [saveMode, setSaveMode] = createSignal<CompositionStudioSaveMode>(
      { kind: "create" },
    );
    const targetDraft = createCompositionGraphDraft("Saved target");
    render(() => (
      <CompositionStudio
        spaceId="space-1"
        initialDraft={targetDraft}
        saveMode={saveMode()}
        backHref="/spaces"
        backLabel="Back"
      />
    ));
    const name = await screen.findByRole("textbox", { name: "Name" });
    fireEvent.input(name, { target: { value: "create-local" } });
    setSaveMode({
      kind: "update",
      compositionId: "composition-2",
      baseRevisionId: "revision-2",
    });
    await waitFor(() => expect(name).toHaveValue("Saved target"));

    fireEvent.input(name, { target: { value: "old-principal-local" } });
    authSessionMock.mockResolvedValue({
      authenticated: true,
      account: { account_id: "editor-2", display_name: "Editor 2" },
    });
    fireEvent(window, new Event("focus"));
    await waitFor(() => expect(name).toHaveValue("Saved target"));
    expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("offers only Design and Variables and keeps Table controls in the canvas", async () => {
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
    expect(screen.queryByRole("button", { name: "Metric" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Saved SQL" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    const dialog = screen.getByRole("dialog", { name: "Table" });
    expect(within(dialog).queryByRole("tab", { name: "Saved SQL" }))
      .toBeNull();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "rows" }),
    );

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const designCanvas = screen.getByRole("region", { name: "Design" });
    expect(
      designCanvas.querySelector(
        '[data-component-select="true"][aria-label="rows"]',
      ),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Change variable, rows" }),
    ).toBeVisible();
  });

  it("creates and places an Input from Design when the graph has no inputs", async () => {
    previewGraphMock.mockResolvedValueOnce({
      ok: true,
      plan: { queries: [] },
    });
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));
    fireEvent.input(screen.getByRole("textbox", { name: "Variable name" }), {
      target: { value: "customer_id" },
    });
    fireEvent.submit(
      screen.getByRole("textbox", { name: "Variable name" })
        .closest("form")!,
    );

    expect(await screen.findByRole("combobox", { name: "Type" }))
      .toBeVisible();
    expect(screen.getByRole("tab", { name: "Variables" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("textbox", { name: "customer_id" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    const saveCall = screen.getByRole("button", { name: "Save" });
    expect(saveCall).toBeDisabled();
    fireEvent.change(screen.getByRole("combobox", { name: "Type" }), {
      target: { value: "integer" },
    });
    fireEvent.input(screen.getByRole("textbox", { name: "Default" }), {
      target: { value: "42" },
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Use" }))
        .toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Use" }));
    expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await screen.findByRole("textbox", { name: "customer_id" }))
      .toBeVisible();
    expect(saveCall).toBeEnabled();
    fireEvent.click(saveCall);
    await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledOnce());

    const spec = (canonicalizeMock.mock.calls[0][0] as {
      spec: CompositionGraphDraft;
    }).spec;
    expect(spec.variables).toContainEqual(expect.objectContaining({
      id: "customer_id",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      default: { kind: "integer", value: 42 },
    }));
    expect(spec.components).toContainEqual({
      id: "input_1",
      kind: "input",
      variable: "customer_id",
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

  it("parks an uncertain retry under its original authenticated principal", async () => {
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
    renderStudio(renderedDraftWithTable());
    fireEvent.click(await screen.findByRole("button", { name: "Save" }));
    await screen.findByRole("button", { name: "Retry" });

    authSessionMock.mockResolvedValue({
      authenticated: true,
      account: { account_id: "editor-2", display_name: "Other editor" },
    });
    window.dispatchEvent(new Event("focus"));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull()
    );
    expect(saveMock).toHaveBeenCalledOnce();

    authSessionMock.mockResolvedValue({
      authenticated: true,
      account: { account_id: "editor-1", display_name: "Editor" },
    });
    window.dispatchEvent(new Event("focus"));
    const retry = await screen.findByRole("button", { name: "Retry" });
    fireEvent.click(retry);
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));
    expect(saveMock.mock.calls[1]).toEqual(saveMock.mock.calls[0]);
  });

  it("does not dispatch a preflight save after its principal changes", async () => {
    let finishCanonicalize!: (result: { canonical_yaml: string }) => void;
    canonicalizeMock.mockImplementationOnce(() =>
      new Promise((resolve) => finishCanonicalize = resolve)
    );
    renderStudio(renderedDraftWithTable());
    fireEvent.click(await screen.findByRole("button", { name: "Save" }));
    await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledOnce());

    authSessionMock.mockResolvedValue({
      authenticated: true,
      account: { account_id: "editor-2", display_name: "Other editor" },
    });
    window.dispatchEvent(new Event("focus"));
    await waitFor(() =>
      expect(document.querySelector(".compositionGraphStudioEditor"))
        .not.toHaveAttribute("inert")
    );
    finishCanonicalize({
      canonical_yaml: JSON.stringify(canonicalizeMock.mock.calls[0][0]),
    });

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(saveMock).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
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
    await waitFor(() => expect(save).toBeDisabled());
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

  it("keeps an already-sent save bound to its original route for retry", async () => {
    let rejectSave!: (error: unknown) => void;
    saveMock.mockImplementationOnce(() =>
      new Promise((_resolve, reject) => rejectSave = reject)
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
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    const originalCall = saveMock.mock.calls[0];
    routePathMock.value = "/spaces/space-1/compositions/composition-2/edit";
    setSaveMode({
      kind: "update",
      compositionId: "composition-2",
      baseRevisionId: "revision-2",
    });
    rejectSave(
      Object.assign(new Error("response lost"), {
        mutationOutcome: "unknown",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull()
    );

    routePathMock.value = "/spaces/space-1/compositions/new";
    setSaveMode({ kind: "create" });
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

  it("allows a new scope save before an old preflight resolves", async () => {
    let finishCanonicalize!: (result: { canonical_yaml: string }) => void;
    let finishOtherSave!: (result: {
      composition_id: string;
      revision_id: string;
    }) => void;
    canonicalizeMock.mockImplementationOnce(() =>
      new Promise((resolve) => finishCanonicalize = resolve)
    );
    const [spaceId, setSpaceId] = createSignal("space-1");
    const [initialDraft, setInitialDraft] = createSignal(
      renderedDraftWithTable(),
    );
    const [saveMode, setSaveMode] = createSignal<CompositionStudioSaveMode>({
      kind: "update",
      compositionId: "composition-1",
      baseRevisionId: "revision-1",
    });
    routePathMock.value = "/spaces/space-1/compositions/composition-1/edit";
    render(() => (
      <CompositionStudio
        spaceId={spaceId()}
        initialDraft={initialDraft()}
        saveMode={saveMode()}
        backHref="/spaces/space-1/compositions"
        backLabel="Back to compositions"
      />
    ));

    fireEvent.click(await screen.findByRole("button", { name: "Save" }));
    await waitFor(() => expect(canonicalizeMock).toHaveBeenCalledOnce());
    routePathMock.value = "/spaces/space-2/compositions/composition-2/edit";
    setSpaceId("space-2");
    setInitialDraft({
      ...renderedDraftWithTable(),
      name: "Other Space Report",
    });
    setSaveMode({
      kind: "update",
      compositionId: "composition-2",
      baseRevisionId: "revision-2",
    });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Name" }))
        .toHaveValue("Other Space Report")
    );
    saveMock.mockImplementationOnce(() =>
      new Promise((resolve) => finishOtherSave = resolve)
    );
    const save = screen.getByRole("button", { name: "Save" });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    expect(saveMock.mock.calls[0][0]).toBe("space-2");
    expect(JSON.parse(saveMock.mock.calls[0][1]).name)
      .toBe("Other Space Report");
    expect(document.querySelector(".compositionGraphStudioRoot"))
      .toHaveAttribute("aria-busy", "true");

    routePathMock.value = "/spaces/space-1/compositions/composition-1/edit";
    setSpaceId("space-1");
    setInitialDraft(renderedDraftWithTable());
    setSaveMode({
      kind: "update",
      compositionId: "composition-1",
      baseRevisionId: "revision-1",
    });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Name" }))
        .toHaveValue("Report")
    );
    const originalScopeSave = screen.getByRole("button", { name: "Save" });
    await waitFor(() => expect(originalScopeSave).toBeEnabled());
    fireEvent.click(originalScopeSave);
    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(2));

    finishCanonicalize({
      canonical_yaml: JSON.stringify(canonicalizeMock.mock.calls[0][0]),
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(saveMock).toHaveBeenCalledTimes(2);
    expect(navigateMock).toHaveBeenCalledOnce();

    finishOtherSave({
      composition_id: "composition-2",
      revision_id: "revision-3",
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(navigateMock).toHaveBeenCalledOnce();
    expect(navigateMock).toHaveBeenCalledWith(
      "/spaces/space-1/compositions/composition-1/revision-1",
    );
  });

  it("edits the shared graph through the Variables mode", async () => {
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "Input" }));

    fireEvent.input(screen.getByRole("textbox", { name: "Variable name" }), {
      target: { value: "maximum_rows" },
    });
    fireEvent.submit(
      screen.getByRole("textbox", { name: "Variable name" })
        .closest("form")!,
    );
    expect(screen.getByRole("textbox", { name: "Variable name" })).toHaveValue(
      "maximum_rows",
    );
    expect(
      screen.queryByText("SQL", { selector: ".compositionVariableRailItem" }),
    ).toBeNull();
  });

  it("writes inline SQL template source back to the graph node", async () => {
    renderStudio(createCompositionGraphDraft("Report"));
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "Add variable" }));
    fireEvent.click(screen.getByRole("button", { name: "SQL" }));

    fireEvent.input(screen.getByRole("textbox", { name: "Variable name" }), {
      target: { value: "query" },
    });
    fireEvent.submit(
      screen.getByRole("textbox", { name: "Variable name" })
        .closest("form")!,
    );

    const editor = screen.getByRole("textbox", { name: "SQL" });
    expect(editor).toHaveValue("SELECT 1");
    fireEvent.input(editor, { target: { value: "SELECT {{minimum}}" } });
    expect(editor).toHaveValue("SELECT {{minimum}}");
  });

  it("keeps Save disabled for an invalid literal after another variable is valid", async () => {
    const draft = renderedDraftWithTable();
    draft.variables.unshift(
      {
        id: "first",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: "1" },
      },
      {
        id: "second",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: "2" },
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
        value: { kind: "integer", value: "1" },
      },
      {
        id: "replacement",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: "2" },
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
          sql: "SELECT {{limit}}",
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
                  value: { kind: "integer", value: "99" },
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
    await checkQuery();
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());

    fireEvent.change(
      screen.getByRole("combobox", { name: "Choose a variable for limit" }),
      {
        target: { value: "replacement" },
      },
    );
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
        default: { kind: "integer", value: "1" },
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
                  value: { kind: "integer", value: "99" },
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
    await checkQuery();
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
        value: { kind: "integer", value: "1" },
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
    await checkQuery();
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
    await checkQuery();
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));

    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
    expect(evaluateMock.mock.calls[0][1]).toMatchObject({
      target: { kind: "draft" },
      requested_queries: ["query_1"],
    });
    expect(evaluateMock.mock.calls[0][1]).not.toHaveProperty("query_results");
  });

  it("passes the current typed Input Work snapshot to Run without saving it", async () => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [
      {
        id: "limit",
        kind: "input",
        value_type: { scalar_type: "integer", nullable: false },
        required: true,
      },
      {
        id: "rows",
        kind: "query",
        returns: { kind: "table", columns: [] },
        query: {
          kind: "inline_sql",
          sql: "SELECT {{limit}} AS value",
          arguments: [{ name: "limit", variable: "limit" }],
        },
      },
    ];
    draft.components = [{ id: "input_1", kind: "input", variable: "limit" }];
    draft.layout = {
      kind: "flow",
      rows: [{
        id: "main",
        items: [{ kind: "component", component: "input_1" }],
      }],
    };
    renderStudio(draft);
    const input = await screen.findByRole("textbox", { name: "limit" });
    fireEvent.input(input, { target: { value: "7" } });
    await waitFor(() => expect(parseInputMock).toHaveBeenCalled());
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await checkQuery();

    const callsBeforeRun = evaluateMock.mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() =>
      expect(evaluateMock.mock.calls.length).toBeGreaterThan(callsBeforeRun)
    );
    const [request, _signal, integerVariableIds] = evaluateMock.mock.calls
      .filter(([, value]) => value.requested_queries?.includes("rows"))
      .at(-1)!.slice(1);
    expect(request.variables).toEqual({ limit: 7 });
    expect(integerVariableIds).toContain("limit");
    expect(request).not.toHaveProperty("component_values");
    expect(saveMock).not.toHaveBeenCalled();
    expect(draft.variables.find((variable) => variable.id === "limit"))
      .not.toHaveProperty("value");
  });

  it("requires a current Check and discards a delayed result after SQL changes", async () => {
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
    let resolveCheck!: (value: unknown) => void;
    previewGraphMock.mockImplementationOnce((_space: string, _yaml: string) =>
      new Promise((resolve) => {
        resolveCheck = (value) => resolve(value);
      })
    );
    renderStudio(draft);
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "query_1" }));
    expect(screen.getByRole("button", { name: "Run query" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Check" }));
    await waitFor(() => expect(previewGraphMock).toHaveBeenCalledOnce());
    expect(previewGraphMock.mock.calls[0][6]).toEqual(["query_1"]);
    const staleYaml = previewGraphMock.mock.calls[0][1];

    fireEvent.input(screen.getByRole("textbox", { name: "SQL" }), {
      target: { value: "SELECT 2" },
    });
    expect(screen.getByRole("button", { name: "Run query" })).toBeDisabled();
    resolveCheck(plannedQuery(staleYaml, {
      kind: "scalar",
      value_type: { scalar_type: "integer", nullable: false },
    }));
    await Promise.resolve();
    expect(screen.getByRole("button", { name: "Run query" })).toBeDisabled();
    expect(evaluateMock).not.toHaveBeenCalled();

    await checkQuery();
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() => expect(evaluateMock).toHaveBeenCalledOnce());
  });

  it("discards a rejected principal lookup from an obsolete Run after SQL changes", async () => {
    renderStudio();
    fireEvent.click(await screen.findByRole("tab", { name: "Variables" }));
    fireEvent.click(screen.getByRole("button", { name: "rows" }));
    await checkQuery();

    let rejectPrincipal!: (error: Error) => void;
    authSessionMock.mockClear();
    authSessionMock.mockImplementationOnce(() =>
      new Promise((_, reject) => {
        rejectPrincipal = reject;
      })
    );
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    await waitFor(() => expect(authSessionMock).toHaveBeenCalledOnce());

    fireEvent.input(screen.getByRole("textbox", { name: "SQL" }), {
      target: { value: "SELECT 2" },
    });
    expect(screen.getByRole("button", { name: "Run query" })).toBeDisabled();
    rejectPrincipal(new Error("session lookup failed"));
    await Promise.resolve();

    expect(screen.getByTestId("query-preview-status"))
      .not.toHaveTextContent("error");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(evaluateMock).not.toHaveBeenCalled();
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
          amount: projectableField(
            { type: "float", required: false, sql_column: "amount" },
            "amount",
            1,
          ),
        },
      },
      {
        id: "form-id-b",
        name: "Expenses",
        sql_relation: "form_expenses",
        version: 1,
        template: "",
        fields: {
          amount: projectableField(
            { type: "float", required: false, sql_column: "amount" },
            "amount",
            1,
          ),
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
        'SELECT "amount" AS "amount" FROM "Receipts" ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"',
      )
    );
    expect(
      JSON.parse(screen.getByTestId("host-draft").textContent!).variables
        .find((node: { id: string }) => node.id === "rows").query,
    ).toMatchObject({
      sql:
        'SELECT "amount" AS "amount" FROM "Receipts" ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"',
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
          sql:
            'SELECT "amount" AS "amount" FROM "Receipts" ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"',
          form_bindings: [{ relation: "Receipts", form_id: "form-id" }],
        },
      });

    expect(screen.getByRole("button", { name: "Form" })).toBeDisabled();
  });

  it("saves authored SQL with the selected Form identity in a Table component", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "Receipts",
      sql_relation: "form_receipts",
      fields: {
        amount: {
          ...projectableField(
            {
              type: "float",
              required: false,
              sql_column: "amount",
              label: "Amount paid",
            },
            "amount",
            1,
          ),
        },
      },
    }]);
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "table",
        columns: [{
          name: "value",
          value_type: { scalar_type: "integer", nullable: false },
        }],
      })
    );
    renderStudio(createCompositionGraphDraft("Report"));
    const editor = await createTableQueryFromDesign("receipts_rows");
    await selectFormForQuery("Receipts");
    const sql =
      'SELECT "amount" AS "Amount paid" FROM "Receipts" ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"';
    await waitFor(() => expect(editor).toHaveValue(sql));
    fireEvent.click(screen.getByRole("button", { name: "Use", exact: true }));
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Design" })).toHaveAttribute(
        "aria-selected",
        "true",
      )
    );
    const saveButton = screen.getByRole("button", {
      name: "Save",
      exact: true,
    });
    await waitFor(() => expect(saveButton).toBeEnabled());
    fireEvent.click(saveButton);
    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    const savedSpec = (canonicalizeMock.mock.calls.at(-1)![0] as {
      spec: {
        variables: Array<{
          id: string;
          query: {
            kind: string;
            sql: string;
            form_bindings: Array<{ relation: string; form_id: string }>;
          };
        }>;
        components: Array<{ id: string; kind: string; variable: string }>;
        layout: { rows: Array<{ items: unknown[] }> };
      };
    }).spec;
    expect(savedSpec.variables[0]).toMatchObject({
      id: "receipts_rows",
      query: {
        kind: "inline_sql",
        sql,
        form_bindings: [{ relation: "Receipts", form_id: "form-id" }],
      },
    });
    expect(savedSpec.components).toEqual([{
      id: "table_1",
      kind: "table",
      variable: "receipts_rows",
    }]);
    expect(savedSpec.layout.rows[0].items).toEqual([{
      kind: "component",
      component: "table_1",
    }]);
  });

  it("keeps an empty Form selection local without rewriting the SQL", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "System records",
      sql_relation: "form_system_records",
      fields: {},
    }]);
    renderStudio(createCompositionGraphDraft("Report"));
    const editor = await createTableQueryFromDesign("system_records_rows");
    await selectFormForQuery("System records");
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
    expect(editor).toHaveValue("SELECT 1");
  });

  it("saves the current planned Table schema without a presentation projection", async () => {
    const initial = renderedDraftWithTable();
    initial.variables[0] = {
      ...initial.variables[0],
      returns: {
        kind: "table",
        columns: [{
          name: "amount",
          value_type: { scalar_type: "float", nullable: false },
        }],
      },
      query: {
        kind: "inline_sql",
        sql: 'SELECT amount FROM "Receipts"',
        arguments: [],
        form_bindings: [{ relation: "Receipts", form_id: "form-id" }],
        schema_fingerprint: "old-schema",
      },
    };
    previewGraphMock.mockImplementation((_spaceId: string, yaml: string) =>
      plannedQuery(yaml, {
        kind: "table",
        columns: [{
          name: "total",
          value_type: { scalar_type: "float", nullable: false },
        }],
      })
    );
    renderStudio(initial);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(saveMock).toHaveBeenCalledOnce());
    expect(previewGraphMock).toHaveBeenCalledOnce();
    const savedSpec = (canonicalizeMock.mock.calls[0][0] as {
      spec: {
        variables: Array<{
          id: string;
          returns: { kind: string; columns?: Array<{ name: string }> };
        }>;
        components: Array<{ presentation?: unknown }>;
      };
    }).spec;
    expect(savedSpec.variables[0]).toMatchObject({
      id: "rows",
      returns: { kind: "table", columns: [{ name: "total" }] },
    });
    expect(savedSpec.components[0]).not.toHaveProperty("presentation");
  });

  it("edits a Form-backed query before the Composition is named without enabling Save", async () => {
    formsMock.mockResolvedValue([{
      id: "form-id",
      name: "Receipts",
      sql_relation: "form_receipts",
      fields: {
        amount: projectableField(
          {
            type: "float",
            required: false,
            sql_column: "amount",
          },
          "amount",
          1,
        ),
      },
    }]);
    renderStudio(createCompositionGraphDraft(""));
    const editor = await createTableQueryFromDesign("receipts_rows");
    await selectFormForQuery("Receipts");
    await waitFor(() =>
      expect(editor).toHaveValue(
        'SELECT "amount" AS "amount" FROM "Receipts" ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"',
      )
    );
    expect(screen.getByRole("textbox", { name: "Name", exact: true }))
      .toHaveValue("");
    expect(screen.getByRole("button", { name: "Save", exact: true }))
      .toBeDisabled();
    expect(saveMock).not.toHaveBeenCalled();
  });
});
