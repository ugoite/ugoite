import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceQueryCreateRoute from "./new";
import { expectBackLinkAtHeaderStart } from "~/test/back-link-placement";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import { setLocale, t } from "~/lib/i18n";
import type { SqlStructureAnalysis } from "~/lib/ugoite-client";
import type { Form } from "~/lib/types";

const {
  navigateMock,
  formApiListMock,
  sqlCreateMock,
  sqlQueryMock,
  sqlCountMock,
  analyzeSqlStructureMock,
  applySqlDocumentEditTransactionMock,
  applySqlDocumentNodeEditTransactionMock,
  applySqlDocumentReplacementTransactionMock,
  parseSqlDocumentMock,
  prepareSqlStructureParserMock,
  proposeSqlStructureOperationMock,
} = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  formApiListMock: vi.fn(),
  sqlCreateMock: vi.fn(),
  sqlQueryMock: vi.fn(),
  sqlCountMock: vi.fn(),
  analyzeSqlStructureMock: vi.fn(),
  applySqlDocumentEditTransactionMock: vi.fn(),
  applySqlDocumentNodeEditTransactionMock: vi.fn(),
  applySqlDocumentReplacementTransactionMock: vi.fn(),
  parseSqlDocumentMock: vi.fn(),
  prepareSqlStructureParserMock: vi.fn(),
  proposeSqlStructureOperationMock: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  A: (props: {
    href: string;
    class?: string;
    children: unknown;
    "aria-label"?: string;
    title?: string;
  }) => (
    <a
      href={props.href}
      class={props.class}
      aria-label={props["aria-label"]}
      title={props.title}
    >
      {props.children}
    </a>
  ),
  useNavigate: () => navigateMock,
  useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/components/SQLWorkspace", () => ({
  SQLWorkspaceActionBar: (props: {
    actions?: readonly SQLWorkspaceAction[];
  }) => (
    <div role="group" aria-label="SQL">
      {props.actions?.map((action) => {
        const accessibleName = action.disabled && action.disabledReason
          ? `${action.label}: ${action.disabledReason}`
          : action.label;
        return (
          <button
            type="button"
            aria-label={accessibleName}
            title={accessibleName}
            disabled={action.disabled}
            onClick={action.onActivate}
          >
            {action.label}
          </button>
        );
      })}
    </div>
  ),
  SQLWorkspace: (props: {
    id?: string;
    sql: string;
    onSqlChange?: (value: string) => void;
    onPreview: () => void;
    onDiagnostics?: (
      diagnostics: Array<{
        from: number;
        to: number;
        severity: "error";
        message: string;
      }>,
    ) => void;
    preview: { status: string };
    hideActions?: boolean;
  }) => {
    const emitBackendDiagnostic = (value: string) => {
      props.onDiagnostics?.(
        value.includes("missing_relation")
          ? [{
            from: 0,
            to: value.length,
            severity: "error",
            message: "Backend rejected the relation",
          }]
          : [],
      );
    };
    return (
      <div
        data-testid="sql-workspace"
        data-hide-actions={props.hideActions ? "true" : "false"}
      >
        <section class="sql-workspace__editor" aria-label="SQL">
          <textarea
            id={props.id}
            aria-label="SQL"
            value={props.sql}
            onInput={(event) => {
              const value = event.currentTarget.value;
              props.onSqlChange?.(value);
              emitBackendDiagnostic(value);
            }}
          />
        </section>
        <button type="button" onClick={props.onPreview}>Run Query</button>
        <output>{props.preview.status}</output>
        {props.preview.status === "success" && (
          <section
            class="sql-workspace__results"
            aria-label="SQL results"
          >
            Rows
          </section>
        )}
      </div>
    );
  },
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: { list: formApiListMock },
  sqlApi: {
    create: sqlCreateMock,
    query: sqlQueryMock,
    count: sqlCountMock,
  },
  analyzeSqlStructure: analyzeSqlStructureMock,
  applySqlDocumentEditTransaction: applySqlDocumentEditTransactionMock,
  applySqlDocumentNodeEditTransaction: applySqlDocumentNodeEditTransactionMock,
  applySqlDocumentReplacementTransaction: applySqlDocumentReplacementTransactionMock,
  parseSqlDocument: parseSqlDocumentMock,
  prepareSqlStructureParser: prepareSqlStructureParserMock,
  proposeSqlStructureOperation: proposeSqlStructureOperationMock,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

const supportedAnalysis: SqlStructureAnalysis = {
  contract_version: 1,
  status: "supported",
  projection: ["1"],
  filters: [],
  aggregates: [],
  group_by: [],
  distinct: false,
  order_by: [],
  referenced_variables: [],
  stable_pagination: false,
  capabilities: {
    columns: { enabled: false },
    filter: { enabled: false },
    sort: { enabled: false },
    aggregate: { enabled: false },
    group_by: { enabled: false },
    distinct: { enabled: false },
    stable_pagination: { enabled: false },
  },
};

describe("/spaces/:space_id/sql/new", () => {
  beforeEach(() => {
    setLocale("en");
    navigateMock.mockReset();
    parseSqlDocumentMock.mockReset().mockImplementation(async (source: string) => ({
      contract_version: 1,
      source,
      status: "parsed",
      diagnostics: [],
      tokens: [],
      nodes: [],
    }));
    formApiListMock.mockResolvedValue([
      {
        id: "sql-form",
        name: "SQL",
        sql_relation: "form_sql",
        version: 1,
        template: "",
        fields: {},
      },
      {
        id: "user-form",
        name: "User",
        sql_relation: "form_user",
        version: 1,
        template: "",
        fields: {},
      },
      {
        id: "group-form",
        name: "UserGroup",
        sql_relation: "form_group",
        version: 1,
        template: "",
        fields: {},
      },
      {
        id: "form-1",
        name: "Entry",
        sql_relation: "form_entry",
        version: 1,
        template: "# Entry\n",
        fields: {},
      },
      {
        id: "review-form",
        name: "Review",
        sql_relation: "form_review",
        version: 1,
        template: "# Review\n",
        fields: {
          Amount: {
            id: 1,
            type: "integer",
            required: false,
            label: "Amount",
            sql_column: "column_amount",
            query_capability: {
              field: { kind: "property", field_id: 1 },
              name: "Amount",
              field_type: "integer",
              filterable: true,
              sortable: true,
              projectable: true,
              supported_operators: ["equals"],
            },
          },
          Category: {
            id: 2,
            type: "string",
            required: false,
            label: "Category",
            sql_column: "column_category",
            query_capability: {
              field: { kind: "property", field_id: 2 },
              name: "Category",
              field_type: "string",
              filterable: true,
              sortable: true,
              projectable: true,
              supported_operators: ["equals", "contains"],
            },
          },
          "00000000-0000-4000-8000-000000000003": {
            id: 3,
            type: "string",
            required: false,
            sql_column: "column_hidden",
            query_capability: {
              field: { kind: "property", field_id: 3 },
              name: "Field 3",
              field_type: "string",
              filterable: true,
              sortable: true,
              projectable: true,
              supported_operators: ["equals"],
            },
          },
        },
      },
    ]);
    sqlCreateMock.mockResolvedValue({ id: "query-1", revisionId: "rev-1" });
    sqlQueryMock.mockReset().mockResolvedValue({
      columns: ["value"],
      rows: [[1]],
      has_more: false,
      result_schema: [{ name: "value", type: "integer" }],
    });
    sqlCountMock.mockReset().mockResolvedValue(1);
    applySqlDocumentEditTransactionMock.mockReset().mockImplementation(
      async (_source: string, transaction: {
        edits: { replacement: string }[];
      }) => ({
        document: {
          status: "parsed",
          source: transaction.edits[0].replacement,
        },
        undo: transaction,
      }),
    );
    applySqlDocumentNodeEditTransactionMock.mockReset().mockImplementation(
      async (_source: string, transaction: { edits: { replacement: string }[] }) => ({
        document: await parseSqlDocumentMock(transaction.edits[0].replacement),
        undo: transaction,
      }),
    );
    applySqlDocumentReplacementTransactionMock.mockReset().mockImplementation(
      async (_source: string, replacementSource: string) => ({
        document: await parseSqlDocumentMock(replacementSource),
        undo: { expected_source: replacementSource, edits: [] },
      }),
    );
    analyzeSqlStructureMock.mockReset().mockImplementation(
      async (_sql: string, context: { relation: string }) => ({
        ...(context.relation
          ? { ...supportedAnalysis, status: "unsupported" as const }
          : supportedAnalysis),
      }),
    );
    prepareSqlStructureParserMock.mockReset().mockResolvedValue(undefined);
    proposeSqlStructureOperationMock.mockReset().mockImplementation(
      async (
        baseSql: string,
        _context: { relation: string },
        operation: { kind: string; relation?: string },
      ) => ({
        contract_version: 1,
        status: "supported",
        base_sql: baseSql,
        proposed_sql: operation.kind === "set_relation"
          ? `SELECT * FROM "${operation.relation}"`
          : baseSql,
        requires_confirmation: true,
        added_stable_tie_breakers: [],
      }),
    );
  });

  it("keeps Form disabled after SQL analysis until Form options resolve", async () => {
    const pendingForms = deferred<Form[]>();
    const pendingAnalysis = deferred<SqlStructureAnalysis>();
    let formsResolved = false;
    let analysisResolved = false;
    formApiListMock.mockReturnValue(pendingForms.promise);
    analyzeSqlStructureMock.mockImplementationOnce(() =>
      pendingAnalysis.promise.then((result) => {
        analysisResolved = true;
        return result;
      })
    );

    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    const analyzingName = `Form: ${t("sqlPage.structureAnalyzing")}`;
    const formAction = await screen.findByRole("button", {
      name: analyzingName,
    });
    await waitFor(() => {
      expect(analyzeSqlStructureMock).toHaveBeenCalledWith(
        "SELECT 1",
        expect.objectContaining({ relation: "" }),
      );
    });
    expect(formApiListMock).toHaveBeenCalledOnce();
    expect(formsResolved).toBe(false);
    expect(formAction).toBeDisabled();

    pendingAnalysis.resolve(supportedAnalysis);
    const optionsUnavailableName = `Form: ${t("sqlPage.structureUnavailable")}`;
    const disabledForMissingOptions = await screen.findByRole("button", {
      name: optionsUnavailableName,
    });
    expect(analysisResolved).toBe(true);
    expect(formsResolved).toBe(false);
    expect(disabledForMissingOptions).toBeDisabled();

    const forms = [{
      id: "review-form",
      name: "Review",
      sql_relation: "form_review",
      version: 1,
      template: "# Review\n",
      fields: {},
    }];
    formsResolved = true;
    pendingForms.resolve(forms);
    await waitFor(() =>
      expect(analyzeSqlStructureMock).toHaveBeenCalledTimes(3)
    );
    const enabledFormAction = await screen.findByRole("button", {
      name: "Form",
    });
    await waitFor(() => expect(enabledFormAction).toBeEnabled());
    fireEvent.click(enabledFormAction);
    const dialog = await screen.findByRole("dialog", { name: "Form" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Review" }));

    await waitFor(() => {
      expect(proposeSqlStructureOperationMock).toHaveBeenCalledWith(
        "SELECT 1",
        expect.objectContaining({ relation: "form_review" }),
        { kind: "set_relation", relation: "form_review" },
      );
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() =>
      expect(editor).toHaveValue('SELECT * FROM "form_review"')
    );
  });

  it("previews the current draft through stateless SQL without creating Saved SQL", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => expect(editor).toHaveValue("SELECT 1"));
    fireEvent.click(screen.getByRole("button", { name: "Run Query" }));

    await waitFor(() => {
      expect(sqlQueryMock).toHaveBeenCalledWith("default", {
        sql: "SELECT 1",
        parameters: {},
        parameter_types: {},
        include_exact_integer_values: true,
        limit: 100,
      }, expect.any(AbortSignal));
    });
    expect(sqlCreateMock).not.toHaveBeenCalled();
    expect(await screen.findByText("success")).toBeInTheDocument();

    const actionGroup = screen.getByRole("group", { name: "SQL" });
    const formAction = await screen.findByRole("button", { name: "Form" });
    const results = screen.getByRole("region", { name: "SQL results" });
    expect(formAction).toBeInTheDocument();
    expect(
      actionGroup.compareDocumentPosition(editor) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      actionGroup.compareDocumentPosition(results) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(screen.getByTestId("sql-workspace")).toHaveAttribute(
      "data-hide-actions",
      "true",
    );
    fireEvent.click(formAction);
    expect(await screen.findByRole("dialog", { name: "Form" }))
      .toBeInTheDocument();
  });

  it("REQ-FE-063: starts from a relation-free draft and saves through the shared SQL workspace", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => expect(editor).toHaveValue("SELECT 1"));
    expect(screen.queryByLabelText("Form for this query")).not
      .toBeInTheDocument();
    expect(screen.queryByLabelText("Aggregate")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Field")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Filter field")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Generate SQL from Form" }))
      .not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Generate aggregate SQL" }))
      .not.toBeInTheDocument();
    expect(screen.getByRole("group", { name: "SQL" })).toBeInTheDocument();
    await waitFor(() => expect(formApiListMock).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Form" })).toBeEnabled()
    );
    fireEvent.input(editor, { target: { value: "SELECT 1" } });
    fireEvent.input(screen.getByLabelText("Query name"), {
      target: { value: "Relation-free draft" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: "Relation-free draft",
        kind: "user-query",
        metadata: undefined,
        sql: "SELECT 1",
        variables: [],
      });
    });
    expect(navigateMock).toHaveBeenCalledWith("/spaces/default/sql");
  });

  it("normalizes template variables to the native SQL query placeholder", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue("SELECT 1");
    });
    fireEvent.input(editor, {
      target: {
        value:
          'SELECT * FROM "form_entry" WHERE Body = {{title}} ORDER BY _ugoite_id',
      },
    });
    fireEvent.input(screen.getByLabelText("Query name"), {
      target: { value: "Variable query" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: "Variable query",
        kind: "user-query",
        metadata: undefined,
        sql:
          'SELECT * FROM "form_entry" WHERE Body = $title ORDER BY _ugoite_id',
        variables: [{ type: "string", name: "title", description: "" }],
      });
    });
  });

  it("saves an explicitly enabled default for a SQL variable", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => expect(editor).toHaveValue("SELECT 1"));
    fireEvent.input(editor, { target: { value: "SELECT $label" } });

    fireEvent.click(
      screen.getByRole("checkbox", { name: "Default for label" }),
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Default for label" }),
      { target: { value: "draft" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith(
        "default",
        expect.objectContaining({
          sql: "SELECT $label",
          variables: [{
            type: "string",
            name: "label",
            description: "",
            default: "draft",
          }],
        }),
      );
    });
  });

  it("leaves session semantic validation to the backend", async () => {
    render(() => <SpaceQueryCreateRoute />);
    const editor = await screen.findByRole("textbox", { name: "SQL" });
    await waitFor(() => {
      expect(editor).toHaveValue("SELECT 1");
    });
    fireEvent.input(editor, {
      target: { value: "SELECT * FROM missing_relation LIMIT $page_size" },
    });
    expect(await screen.findByText(/Backend rejected the relation/))
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(sqlCreateMock).toHaveBeenCalledWith("default", {
        name: null,
        kind: "user-query",
        metadata: { generatedName: "untitled" },
        sql: "SELECT * FROM missing_relation LIMIT $page_size",
        variables: [{ type: "string", name: "page_size", description: "" }],
      });
    });
  });

  it("PR6: backs to Saved SQL once and reports diagnostics with line and column", async () => {
    render(() => <SpaceQueryCreateRoute />);

    const back = await screen.findByRole("link", { name: "Back to Saved SQL" });
    expect(back).toHaveAttribute("href", "/spaces/default/sql");
    expect(screen.getAllByRole("link", { name: "Back to Saved SQL" }))
      .toHaveLength(1);
    expectBackLinkAtHeaderStart(back);

    const editor = await screen.findByRole("textbox", { name: "SQL" });
    fireEvent.input(editor, {
      target: { value: "SELECT * FROM missing_relation LIMIT $page_size" },
    });
    expect(await screen.findByText(/Line 1, column \d+: /))
      .toBeInTheDocument();
  });
});
