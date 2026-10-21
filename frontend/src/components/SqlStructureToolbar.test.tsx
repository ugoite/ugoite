import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import {
  buildSqlStructureContexts,
  type SqlStructureAdapterContext,
} from "~/lib/sql";
import type { Form } from "~/lib/types";
import type { SQLWorkspaceAction } from "./SQLWorkspace";
import type {
  SqlDocument,
  SqlStructureAnalysis,
  SqlStructureOperation,
  SqlStructureProposal,
  SqlStructureVariableType,
} from "~/lib/ugoite-client";
import { SqlStructureToolbar } from "./SqlStructureToolbar";

const {
  analyzeMock,
  applyDocumentEditMock,
  applyNodeEditMock,
  applyReplacementMock,
  parseDocumentMock,
  prepareMock,
  proposeMock,
} = vi.hoisted(() => ({
  analyzeMock: vi.fn(),
  applyDocumentEditMock: vi.fn(),
  applyNodeEditMock: vi.fn(),
  applyReplacementMock: vi.fn(),
  parseDocumentMock: vi.fn(),
  prepareMock: vi.fn(),
  proposeMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  analyzeSqlStructure: analyzeMock,
  applySqlDocumentEditTransaction: applyDocumentEditMock,
  applySqlDocumentNodeEditTransaction: applyNodeEditMock,
  applySqlDocumentReplacementTransaction: applyReplacementMock,
  parseSqlDocument: parseDocumentMock,
  prepareSqlStructureParser: prepareMock,
  proposeSqlStructureOperation: proposeMock,
}));

const context: SqlStructureAdapterContext = {
  identity: "inline:source-1:form-1:3",
  relationSelectable: true,
  parser: {
    relation: "Orders",
    columns: [
      { name: "_ugoite_id", type: "string" },
      { name: "amount", type: "integer" },
      { name: "category", type: "string" },
      { name: "private_note", type: "string" },
    ],
    variables: [{ name: "threshold", type: "integer" }],
    stable_key: ["_ugoite_id"],
  },
  columnCapabilities: {
    _ugoite_id: {
      projectable: true,
      filterable: true,
      sortable: true,
      supportedOperators: ["eq"],
      aggregateFunctions: ["count", "min", "max"],
      groupable: true,
      distinctable: true,
    },
    amount: {
      projectable: true,
      filterable: true,
      sortable: true,
      supportedOperators: ["eq", "gt"],
      aggregateFunctions: ["count", "sum", "avg", "mean", "min", "max"],
      groupable: true,
      distinctable: true,
    },
    category: {
      projectable: true,
      filterable: true,
      sortable: true,
      supportedOperators: ["eq"],
      aggregateFunctions: ["count", "min", "max"],
      groupable: true,
      distinctable: true,
    },
    private_note: {
      projectable: false,
      filterable: false,
      sortable: false,
      supportedOperators: [],
      aggregateFunctions: [],
      groupable: false,
      distinctable: false,
    },
  },
};

const analysis: SqlStructureAnalysis = {
  contract_version: 5,
  status: "supported",
  projection: ["*"],
  literal_projection: [],
  projection_aliases: [],
  filters: [],
  aggregates: [],
  group_by: [],
  distinct: false,
  row_limit: {},
  order_by: [{ column: "_ugoite_id", direction: "asc" }],
  joins: [],
  referenced_variables: [],
  stable_pagination: true,
  capabilities: {
    join: { enabled: true },
    columns: { enabled: true },
    filter: { enabled: true },
    filter_expression: { enabled: true },
    sort: { enabled: true },
    aggregate: { enabled: true },
    group_by: { enabled: true },
    distinct: { enabled: true },
    row_limit: { enabled: true },
    stable_pagination: { enabled: true },
  },
};

const joinedAnalysis: SqlStructureAnalysis = {
  ...analysis,
  filters: [{
    column: "amount",
    qualifier: { name: "left_rows", quoted: true },
    operator: "gt",
    parameter: "threshold",
  }],
  order_by: [{
    column: "_ugoite_id",
    qualifier: { name: "right_rows", quoted: true },
    direction: "asc",
  }],
  joins: [{
    relation: "Orders",
    left_alias: { name: "left_rows", quoted: true },
    right_alias: { name: "right_rows", quoted: true },
    left_column: "category",
    right_column: "category",
  }],
  stable_pagination: false,
  capabilities: {
    ...analysis.capabilities,
    columns: { enabled: false, reason: "join_unsupported" },
    aggregate: { enabled: false, reason: "join_unsupported" },
    group_by: { enabled: false, reason: "join_unsupported" },
    distinct: { enabled: false, reason: "join_unsupported" },
    stable_pagination: { enabled: false, reason: "stable_order_required" },
  },
};

describe("SqlStructureToolbar adapter", () => {
  beforeEach(() => {
    setLocale("en");
    analyzeMock.mockReset();
    applyDocumentEditMock.mockReset();
    applyNodeEditMock.mockReset();
    applyReplacementMock.mockReset();
    parseDocumentMock.mockReset();
    prepareMock.mockReset();
    proposeMock.mockReset();
    prepareMock.mockResolvedValue(undefined);
    parseDocumentMock.mockImplementation(async (
      source: string,
    ): Promise<SqlDocument> => ({
      contract_version: 1,
      source,
      status: "parsed",
      diagnostics: [],
      tokens: [],
      nodes: [{
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: new TextEncoder().encode(source).byteLength },
      }],
    }));
    analyzeMock.mockResolvedValue(analysis);
    applyDocumentEditMock.mockImplementation(
      async (_currentSource: string, transaction: {
        edits: { replacement: string }[];
      }) => ({
        document: {
          status: "parsed",
          source: transaction.edits[0].replacement,
        },
        undo: transaction,
      }),
    );
    applyNodeEditMock.mockImplementation(
      async (_currentSource: string, transaction: {
        edits: { replacement: string }[];
      }) => ({
        document: await parseDocumentMock(transaction.edits[0].replacement),
        undo: { expected_source: transaction.edits[0].replacement, edits: [] },
      }),
    );
    applyReplacementMock.mockImplementation(
      async (_currentSource: string, replacementSource: string) => ({
        document: await parseDocumentMock(replacementSource),
        undo: { expected_source: replacementSource, edits: [] },
      }),
    );
    proposeMock.mockImplementation(async (baseSql: string) => ({
      contract_version: 5,
      status: "supported",
      base_sql: baseSql,
      proposed_sql: 'SELECT "amount" FROM "Orders" ORDER BY "_ugoite_id" ASC',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    }));
  });

  it("opens the Rust node tree without Form analysis and applies a guarded node edit", async () => {
    const sql = "SELECT 1";
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => []}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    const structureAction = await waitFor(() => {
      const action = actions.find((item) => item.kind === "structure");
      expect(action?.disabled).toBe(false);
      return action!;
    });
    structureAction.onActivate();
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("navigation")).toBeInTheDocument();

    const replacement = within(dialog).getByRole("textbox", {
      name: "Proposed SQL",
    });
    fireEvent.input(replacement, { target: { value: "SELECT 2" } });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );

    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith("SELECT 2"));
    expect(applyNodeEditMock).toHaveBeenCalledWith("SELECT 1", {
      contract_version: 1,
      expected_source: "SELECT 1",
      edits: [{
        node_id: 0,
        expected_kind: "query_statement",
        replacement: "SELECT 2",
      }],
    });
  });

  it("scopes undo and redo to their document and drops history after direct SQL input", async () => {
    const [sql, setSql] = createSignal("SELECT 1");
    const [scopeKey, setScopeKey] = createSignal("source-a");
    const onSqlChange = vi.fn((nextSql: string) => setSql(nextSql));
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={sql}
        historyScopeKey={scopeKey}
        contexts={() => []}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    const structureAction = await waitFor(() => {
      const action = actions.find((item) => item.kind === "structure");
      expect(action?.disabled).toBe(false);
      return action!;
    });
    structureAction.onActivate();
    const dialog = screen.getByRole("dialog");
    const replacement = within(dialog).getByRole("textbox", {
      name: "Proposed SQL",
    });
    fireEvent.input(replacement, { target: { value: "SELECT 2" } });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(sql()).toBe("SELECT 2"));

    const undo = within(dialog).getByRole("button", {
      name: "Undo structure edit",
    });
    expect(undo).toBeEnabled();
    fireEvent.click(undo);
    await waitFor(() => expect(sql()).toBe("SELECT 1"));

    const redo = within(dialog).getByRole("button", {
      name: "Redo structure edit",
    });
    expect(redo).toBeEnabled();
    fireEvent.click(redo);
    await waitFor(() => expect(sql()).toBe("SELECT 2"));

    // The second source has identical SQL text. Its undo stack must start
    // empty even though the editor instance is reused by the host.
    setScopeKey("source-b");
    expect(sql()).toBe("SELECT 2");
    const secondDialog = screen.queryByRole("dialog") ?? (() => {
      actions.find((item) => item.kind === "structure")!.onActivate();
      return screen.getByRole("dialog");
    })();
    expect(
      within(secondDialog).getByRole("button", {
        name: "Undo structure edit",
      }),
    ).toBeDisabled();
    expect(
      within(secondDialog).getByRole("button", {
        name: "Redo structure edit",
      }),
    ).toBeDisabled();

    setSql("SELECT 3");
    const thirdDialog = screen.queryByRole("dialog") ?? (() => {
      actions.find((item) => item.kind === "structure")!.onActivate();
      return screen.getByRole("dialog");
    })();
    await waitFor(() =>
      expect(
        within(thirdDialog).getByLabelText("Current SQL"),
      ).toHaveTextContent("SELECT 3")
    );
    expect(
      within(thirdDialog).getByRole("button", {
        name: "Undo structure edit",
      }),
    ).toBeDisabled();
    expect(onSqlChange.mock.calls.map(([nextSql]) => nextSql)).toEqual([
      "SELECT 2",
      "SELECT 1",
      "SELECT 2",
    ]);
  });

  it("keeps each edit entry point available through the SQL tree without Form context", async () => {
    const sql = 'SELECT * FROM "G"';
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => []}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() => expect(actions).toHaveLength(8));
    await waitFor(() =>
      expect(
        actions.filter((action) => action.kind !== "form").every((action) =>
          !action.disabled
        ) && actions.find((action) => action.kind === "form")?.disabled,
      )
        .toBe(true)
    );

    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    expect(
      within(dialog).getByRole("navigation", {
        name: "SQL structure actions",
      }),
    ).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Review change" }))
      .not.toBeInTheDocument();

    const replacement = within(dialog).getByRole("textbox", {
      name: "Proposed SQL",
    });
    fireEvent.input(replacement, {
      target: { value: 'SELECT "amount" FROM "G"' },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );

    await waitFor(() =>
      expect(onSqlChange).toHaveBeenCalledWith('SELECT "amount" FROM "G"')
    );
    expect(applyNodeEditMock).toHaveBeenCalledWith(sql, {
      contract_version: 1,
      expected_source: sql,
      edits: [{
        node_id: 0,
        expected_kind: "query_statement",
        replacement: 'SELECT "amount" FROM "G"',
      }],
    });
  });

  it("intersects Form capabilities and changes SQL only after proposal confirmation", async () => {
    const sql = 'SELECT * FROM "Orders" ORDER BY "_ugoite_id"';
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() => expect(actions).toHaveLength(8));
    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    expect(actions.find((action) => action.kind === "filter")?.disabled).toBe(
      false,
    );
    expect(actions.find((action) => action.kind === "aggregate")?.disabled)
      .toBe(false);
    expect(actions.find((action) => action.kind === "group_by")?.disabled)
      .toBe(false);
    expect(actions.find((action) => action.kind === "distinct")?.disabled)
      .toBe(false);

    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    expect(within(dialog).getByLabelText("Column 1")).toHaveValue(
      "_ugoite_id",
    );
    expect(within(dialog).queryByRole("option", { name: "private_note" }))
      .toBeNull();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() =>
      expect(within(dialog).getByText("Proposed SQL")).toBeInTheDocument()
    );
    expect(onSqlChange).not.toHaveBeenCalled();
    expect(proposeMock).toHaveBeenCalledOnce();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() =>
      expect(onSqlChange).toHaveBeenCalledWith(
        'SELECT "amount" FROM "Orders" ORDER BY "_ugoite_id" ASC',
      )
    );
  });

  it("applies toolbar proposals through the Rust source-preserving transaction", async () => {
    const initialSql = '-- 注釈\nSELECT * FROM "Orders" ORDER BY "_ugoite_id"';
    const proposedSql =
      '-- 注釈\nSELECT "amount" FROM "Orders" ORDER BY "_ugoite_id" ASC';
    const [sql, setSql] = createSignal(initialSql);
    const onSqlChange = vi.fn((nextSql: string) => setSql(nextSql));
    let actions: readonly SQLWorkspaceAction[] = [];
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: initialSql,
      proposed_sql: proposedSql,
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });

    render(() => (
      <SqlStructureToolbar
        sql={sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "Apply change" }),
    );

    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith(proposedSql));
    expect(applyReplacementMock).toHaveBeenCalledWith(initialSql, proposedSql);
    await waitFor(() =>
      expect(analyzeMock).toHaveBeenLastCalledWith(proposedSql, context.parser)
    );

    const directlyEditedSql = "-- editor input\nSELECT 1";
    setSql(directlyEditedSql);
    await waitFor(() =>
      expect(analyzeMock).toHaveBeenLastCalledWith(
        directlyEditedSql,
        context.parser,
      )
    );
  });

  it("does not commit a toolbar proposal rejected by SQL Document parsing", async () => {
    const sql = 'SELECT * FROM "Orders"';
    const proposedSql =
      'SELECT * FROM "Orders" WHERE "amount" = $filter_amount';
    const emptyContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:document-rejection",
      parser: { ...context.parser, variables: [] },
    };
    const onSqlChange = vi.fn();
    const onVariablesAdded = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: proposedSql,
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    applyReplacementMock.mockResolvedValue({
      document: { status: "invalid_sql", source: proposedSql },
      undo: {},
    });

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [emptyContext]}
        onSqlChange={onSqlChange}
        onVariablesAdded={onVariablesAdded}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add filter" }));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "Apply change" }),
    );

    await waitFor(() =>
      expect(within(dialog).getByRole("alert")).toBeInTheDocument()
    );
    expect(onVariablesAdded).not.toHaveBeenCalled();
    expect(onSqlChange).not.toHaveBeenCalled();
  });

  it("discards an SQL Document response after the source changes", async () => {
    const initialSql = 'SELECT * FROM "Orders"';
    const proposedSql = 'SELECT "amount" FROM "Orders"';
    const [sql, setSql] = createSignal(initialSql);
    const onSqlChange = vi.fn();
    let resolveEdit!: (result: {
      document: { status: string; source: string };
      undo: unknown;
    }) => void;
    applyReplacementMock.mockImplementation(() =>
      new Promise((resolve) => resolveEdit = resolve)
    );
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(applyReplacementMock).toHaveBeenCalledOnce());

    setSql("SELECT 1");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    resolveEdit({
      document: { status: "parsed", source: proposedSql },
      undo: {},
    });
    await applyReplacementMock.mock.results[0].value;
    expect(onSqlChange).not.toHaveBeenCalled();
  });

  it("discards an SQL Document response after its context changes", async () => {
    const sql = 'SELECT * FROM "Orders"';
    const proposedSql = 'SELECT "amount" FROM "Orders"';
    const [contexts, setContexts] = createSignal<
      readonly SqlStructureAdapterContext[]
    >([context]);
    const onSqlChange = vi.fn();
    let resolveEdit!: (result: {
      document: { status: string; source: string };
      undo: unknown;
    }) => void;
    applyReplacementMock.mockImplementation(() =>
      new Promise((resolve) => resolveEdit = resolve)
    );
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={contexts}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(applyReplacementMock).toHaveBeenCalledOnce());

    setContexts([{ ...context, identity: `${context.identity}:changed` }]);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    resolveEdit({
      document: { status: "parsed", source: proposedSql },
      undo: {},
    });
    await applyReplacementMock.mock.results[0].value;
    expect(onSqlChange).not.toHaveBeenCalled();
  });

  it("edits qualified filters on either side of a supported join", async () => {
    analyzeMock.mockResolvedValue(joinedAnalysis);
    const sql =
      'SELECT * FROM "Orders" AS "left_rows" INNER JOIN "Orders" AS "right_rows" ON "left_rows"."category" = "right_rows"."category" WHERE "left_rows"."amount" > $threshold ORDER BY "right_rows"."_ugoite_id"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    const column = within(dialog).getByRole("combobox", { name: "Column" });
    expect(within(column).getByRole("option", { name: "right_rows.amount" }))
      .toBeInTheDocument();
    fireEvent.change(column, {
      target: { value: '["right_rows",true,"amount"]' },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      {
        kind: "set_filter_expression",
        expression: {
          kind: "predicate",
          column: "amount",
          qualifier: { name: "right_rows", quoted: true },
          operator: "gt",
          parameter: "threshold",
        },
      },
    );
  });

  it("keeps join qualifiers through nested boolean filter edits", async () => {
    analyzeMock.mockResolvedValue(
      {
        ...joinedAnalysis,
        filters: [],
        filter_expression: {
          kind: "or",
          expressions: [
            {
              kind: "predicate",
              column: "amount",
              qualifier: { name: "left_rows", quoted: true },
              operator: "gt",
              parameter: "threshold",
            },
            {
              kind: "not",
              expression: {
                kind: "predicate",
                column: "amount",
                qualifier: { name: "right_rows", quoted: true },
                operator: "eq",
                parameter: "threshold",
              },
            },
          ],
        },
      } satisfies SqlStructureAnalysis,
    );
    const sql =
      'SELECT * FROM "Orders" AS "left_rows" INNER JOIN "Orders" AS "right_rows" ON "left_rows"."category" = "right_rows"."category" WHERE "left_rows"."amount" > $threshold OR NOT ("right_rows"."amount" = $threshold) ORDER BY "right_rows"."_ugoite_id"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    const firstFilter = dialog.querySelector(
      ".sql-structure-filter-node select",
    );
    expect(firstFilter).not.toBeNull();
    fireEvent.change(firstFilter as HTMLSelectElement, {
      target: { value: '["right_rows",true,"amount"]' },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      {
        kind: "set_filter_expression",
        expression: {
          kind: "or",
          expressions: [
            {
              kind: "predicate",
              column: "amount",
              qualifier: { name: "right_rows", quoted: true },
              operator: "gt",
              parameter: "threshold",
            },
            {
              kind: "not",
              expression: {
                kind: "predicate",
                column: "amount",
                qualifier: { name: "right_rows", quoted: true },
                operator: "eq",
                parameter: "threshold",
              },
            },
          ],
        },
      },
    );
  });

  it("edits qualified ORDER BY identifiers on either side of a supported join", async () => {
    analyzeMock.mockResolvedValue(joinedAnalysis);
    const sql =
      'SELECT * FROM "Orders" AS "left_rows" INNER JOIN "Orders" AS "right_rows" ON "left_rows"."category" = "right_rows"."category" WHERE "left_rows"."amount" > $threshold ORDER BY "right_rows"."_ugoite_id"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "sort")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "sort")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Sort" });
    const column = within(dialog).getByRole("combobox", { name: "Column" });
    expect(within(column).getByRole("option", { name: "left_rows._ugoite_id" }))
      .toBeInTheDocument();
    fireEvent.change(column, {
      target: { value: '["left_rows",true,"_ugoite_id"]' },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      {
        kind: "set_sort",
        order_by: [{
          column: "_ugoite_id",
          qualifier: { name: "left_rows", quoted: true },
          direction: "asc",
        }],
      },
    );
  });

  it("shows and rewrites ORDER BY projection aliases as aliases", async () => {
    const alias = { name: "field_101", quoted: true };
    const collisionContext: SqlStructureAdapterContext = {
      ...context,
      parser: {
        ...context.parser,
        columns: [
          ...context.parser.columns,
          { name: "field_101", type: "string" },
          { name: "field_102", type: "integer" },
        ],
      },
      columnCapabilities: {
        ...context.columnCapabilities,
        field_101: context.columnCapabilities.category,
        field_102: context.columnCapabilities.amount,
      },
    };
    analyzeMock.mockResolvedValue({
      ...analysis,
      projection: ["field_102"],
      projection_aliases: [{ column: "field_102", alias }],
      order_by: [{
        column: "field_102",
        projection_alias: alias,
        direction: "asc",
      }],
    });
    const sql =
      'SELECT "field_102" AS "field_101" FROM "Orders" ORDER BY "field_101"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [collisionContext]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "sort")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "sort")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Sort" });
    const column = within(dialog).getByRole("combobox", { name: "Column" });
    const aliasValue = JSON.stringify([
      "projection_alias",
      alias.name,
      alias.quoted,
    ]);
    expect(column).toHaveValue(aliasValue);
    expect(
      within(column).getByRole("option", {
        name: "alias field_101 (field_102)",
      }),
    ).toBeInTheDocument();
    fireEvent.change(
      within(dialog).getByRole("combobox", { name: "Direction" }),
      { target: { value: "desc" } },
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(sql, collisionContext.parser, {
      kind: "set_sort",
      order_by: [{
        column: "field_102",
        projection_alias: alias,
        direction: "desc",
      }],
    });
  });

  it("adds a typed inner self-join through the shared toolbar", async () => {
    const sql = 'SELECT * FROM "Orders"';
    const proposedSql =
      'SELECT * FROM "Orders" AS "_ugoite_left" INNER JOIN "Orders" AS "_ugoite_right" ON "_ugoite_left"."amount" = "_ugoite_right"."amount"';
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: proposedSql,
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    let resolveStructureAction!: (action: SQLWorkspaceAction) => void;
    const structureActionReady = new Promise<SQLWorkspaceAction>((resolve) => {
      resolveStructureAction = resolve;
    });
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => {
          actions = next;
          const structureAction = next.find((action) =>
            action.kind === "structure"
          );
          if (structureAction && !structureAction.disabled) {
            resolveStructureAction(structureAction);
          }
        }}
      />
    ));

    const structureAction = await structureActionReady;
    expect(actions).toHaveLength(8);
    expect(actions.some((action) => action.kind === "join")).toBe(false);
    expect(structureAction.disabled).toBe(false);
    expect(structureAction.label).toBe("Structure");
    expect(structureAction.icon).toBe("group-by");
    structureAction.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Structure" });
    await waitFor(() =>
      expect(within(dialog).getByLabelText("Left column")).toBeInTheDocument()
    );
    fireEvent.change(within(dialog).getByLabelText("Left column"), {
      target: { value: "amount" },
    });
    expect(within(dialog).getByLabelText("Right column")).toHaveValue("amount");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock.mock.calls[0][2]).toEqual({
      kind: "set_join",
      left_column: "amount",
      right_column: "amount",
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith(proposedSql));
  });

  it("edits LIMIT and OFFSET inside the shared Structure action", async () => {
    const sql = 'SELECT * FROM "Orders"';
    const proposedSql = `${sql} LIMIT 25 OFFSET 4`;
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: proposedSql,
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "structure")?.disabled)
        .toBe(false)
    );
    await waitFor(() => expect(analyzeMock).toHaveBeenCalled());
    actions.find((action) => action.kind === "structure")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Structure" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Rows" }));
    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Review change" }),
      ).toBeInTheDocument()
    );
    fireEvent.input(within(dialog).getByLabelText("LIMIT"), {
      target: { value: "25" },
    });
    fireEvent.input(within(dialog).getByLabelText("OFFSET"), {
      target: { value: "4" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock.mock.calls[0][2]).toEqual({
      kind: "set_row_limit",
      limit: "25",
      offset: "4",
      limit_all: false,
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith(proposedSql));
  });

  it("replaces LIMIT ALL through the shared Structure action", async () => {
    const sql = 'SELECT * FROM "Orders" LIMIT ALL';
    const proposedSql = 'SELECT * FROM "Orders" LIMIT 12';
    analyzeMock.mockResolvedValue({
      ...analysis,
      row_limit: { limit_all: true },
    });
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: proposedSql,
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "structure")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "structure")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Structure" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Rows" }));
    await waitFor(() =>
      expect(within(dialog).getByLabelText("LIMIT ALL")).toBeChecked()
    );
    fireEvent.click(within(dialog).getByLabelText("LIMIT ALL"));
    fireEvent.input(within(dialog).getByLabelText("LIMIT"), {
      target: { value: "12" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock.mock.calls[0][2]).toEqual({
      kind: "set_row_limit",
      limit: "12",
      offset: null,
      limit_all: false,
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith(proposedSql));
  });

  it("opens Rows when JOIN is unavailable and shows invalid bounds inline", async () => {
    const sql = 'SELECT * FROM "Orders"';
    analyzeMock.mockResolvedValue({
      ...analysis,
      capabilities: {
        ...analysis.capabilities,
        join: { enabled: false, reason: "join_unsupported" },
      },
    });
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "structure")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "structure")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Structure" });
    await waitFor(() =>
      expect(within(dialog).getByRole("button", { name: "Rows" }))
        .toHaveAttribute("aria-pressed", "true")
    );
    expect(within(dialog).getByRole("button", { name: "Join" })).toBeDisabled();
    fireEvent.input(within(dialog).getByLabelText("LIMIT"), {
      target: { value: "18446744073709551616" },
    });
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "Use non-negative whole numbers",
    );
    expect(
      within(dialog).getByRole("button", { name: /Review change/ }),
    ).toBeDisabled();

    fireEvent.input(within(dialog).getByLabelText("LIMIT"), {
      target: { value: "12" },
    });
    await waitFor(() =>
      expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument()
    );
  });

  it("edits a projection column and alias while preserving its slot id", async () => {
    const sql =
      'SELECT "amount" AS "Total Amount" FROM "Orders" ORDER BY "_ugoite_id"';
    analyzeMock.mockResolvedValue({
      ...analysis,
      projection: ["amount"],
      projection_aliases: [{
        column: "amount",
        alias: { name: "Total Amount", quoted: true },
      }],
    });
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql:
        'SELECT "category" AS GrossTotal FROM "Orders" ORDER BY "_ugoite_id"',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    let actions: readonly SQLWorkspaceAction[] = [];
    const onSqlChange = vi.fn();

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    const aliasInput = within(dialog).getByRole("textbox", {
      name: "Alias for amount at position 1",
    });
    expect(aliasInput).toHaveValue("Total Amount");
    const quoted = within(dialog).getByRole("checkbox", {
      name: "Quote alias for amount at position 1",
    });
    expect(quoted).toBeChecked();
    fireEvent.input(aliasInput, { target: { value: "GrossTotal" } });
    fireEvent.click(quoted);
    fireEvent.change(within(dialog).getByLabelText("Column 1"), {
      target: { value: "category" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      {
        kind: "set_projection_slots",
        slots: [{
          id: "projection-0",
          column: "category",
          alias: { name: "GrossTotal", quoted: false },
        }],
      },
    );
    expect(onSqlChange).not.toHaveBeenCalled();
  });

  it("expands wildcard columns into id-less editable projection slots", async () => {
    const sql = 'SELECT * FROM "Orders"';
    analyzeMock.mockResolvedValue({
      ...analysis,
      projection: ["*"],
      projection_aliases: [],
    });
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql:
        'SELECT "_ugoite_id", "category", "_ugoite_id" AS row_id FROM "Orders"',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    expect(within(dialog).getByLabelText("Column 1")).toHaveValue(
      "_ugoite_id",
    );
    expect(within(dialog).getByLabelText("Column 2")).toHaveValue("amount");
    expect(within(dialog).getByLabelText("Column 3")).toHaveValue("category");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add column" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Remove column 2" }),
    );
    fireEvent.input(
      within(dialog).getByRole("textbox", {
        name: "Alias for _ugoite_id at position 3",
      }),
      { target: { value: "row_id" } },
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      {
        kind: "set_projection_slots",
        slots: [
          { column: "_ugoite_id", alias: null },
          { column: "category", alias: null },
          { column: "_ugoite_id", alias: { name: "row_id", quoted: false } },
        ],
      },
    );
  });

  it("edits and reorders repeated source columns by independent slot ids", async () => {
    const sql =
      'SELECT "amount" AS first, "amount" AS second FROM "Orders" WHERE "category" = $threshold';
    analyzeMock.mockResolvedValue({
      ...analysis,
      projection: ["amount", "amount"],
      projection_aliases: [
        { column: "amount", alias: { name: "first", quoted: false } },
        { column: "amount", alias: { name: "second", quoted: false } },
      ],
    });
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql:
        'SELECT "amount" AS second_copy, "amount" AS first_copy FROM "Orders" WHERE "category" = $threshold',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    const firstAlias = within(dialog).getByRole("textbox", {
      name: "Alias for amount at position 1",
    });
    const secondAlias = within(dialog).getByRole("textbox", {
      name: "Alias for amount at position 2",
    });
    expect(firstAlias).toHaveValue("first");
    expect(secondAlias).toHaveValue("second");
    fireEvent.input(firstAlias, { target: { value: "first_copy" } });
    fireEvent.input(secondAlias, { target: { value: "second_copy" } });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Move column 1 down" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(sql, context.parser, {
      kind: "set_projection_slots",
      slots: [
        {
          id: "projection-1",
          column: "amount",
          alias: { name: "second_copy", quoted: false },
        },
        {
          id: "projection-0",
          column: "amount",
          alias: { name: "first_copy", quoted: false },
        },
      ],
    });
  });

  it("offers a safe structure edit for scalar SELECT without a Form", async () => {
    const sql = "SELECT 1";
    const contexts = buildSqlStructureContexts([], [], "new-sql");
    const scalarAnalysis: SqlStructureAnalysis = {
      contract_version: 5,
      status: "supported",
      projection: ["1"],
      literal_projection: [{ kind: "integer", value: "1" }],
      projection_aliases: [],
      filters: [],
      aggregates: [],
      group_by: [],
      distinct: false,
      row_limit: {},
      order_by: [],
      joins: [],
      referenced_variables: [],
      stable_pagination: false,
      capabilities: {
        join: { enabled: false, reason: "invalid_context" },
        columns: { enabled: true },
        filter: { enabled: false, reason: "invalid_context" },
        sort: { enabled: false, reason: "stable_key_unavailable" },
        aggregate: { enabled: false },
        group_by: { enabled: false },
        distinct: { enabled: true },
        row_limit: { enabled: true },
        stable_pagination: {
          enabled: false,
          reason: "stable_key_unavailable",
        },
      },
    };
    analyzeMock.mockResolvedValue(scalarAnalysis);
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: "SELECT DISTINCT 1",
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => contexts}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "distinct")?.disabled)
        .toBe(false)
    );
    expect(contexts).toHaveLength(1);
    expect(contexts[0].parser).toEqual({
      relation: "",
      columns: [],
      variables: [],
      stable_key: [],
    });
    expect(analyzeMock).toHaveBeenCalledWith(sql, contexts[0].parser);

    actions.find((action) => action.kind === "distinct")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Distinct" });
    fireEvent.click(within(dialog).getByRole("checkbox"));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() =>
      expect(within(dialog).getByText("SELECT DISTINCT 1")).toBeInTheDocument()
    );
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      contexts[0].parser,
      { kind: "set_distinct", distinct: true },
    );
    expect(onSqlChange).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() =>
      expect(onSqlChange).toHaveBeenCalledWith("SELECT DISTINCT 1")
    );
  });

  it("edits a relation-free literal projection through the Columns action", async () => {
    const sql = "SELECT 1";
    const relationFreeContext = buildSqlStructureContexts([], [], "new-sql")[0];
    const scalarAnalysis: SqlStructureAnalysis = {
      ...analysis,
      projection: ["1"],
      literal_projection: [{ kind: "integer", value: "1" }],
      order_by: [],
      stable_pagination: false,
      capabilities: {
        ...analysis.capabilities,
        columns: { enabled: true },
        filter: { enabled: false, reason: "invalid_context" },
        sort: { enabled: false, reason: "stable_key_unavailable" },
        aggregate: { enabled: false },
        group_by: { enabled: false },
        distinct: { enabled: true },
        stable_pagination: {
          enabled: false,
          reason: "stable_key_unavailable",
        },
      },
    };
    analyzeMock.mockResolvedValue(scalarAnalysis);
    proposeMock.mockResolvedValue({
      contract_version: 1,
      status: "supported",
      base_sql: sql,
      proposed_sql: "SELECT 2",
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [relationFreeContext]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    fireEvent.input(within(dialog).getByRole("textbox", { name: "Value 1" }), {
      target: { value: "2" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() =>
      expect(within(dialog).getByText("SELECT 2")).toBeInTheDocument()
    );
    expect(proposeMock).toHaveBeenCalledWith(sql, relationFreeContext.parser, {
      kind: "set_literal_projection",
      values: [{ kind: "integer", value: "2" }],
    });
    expect(onSqlChange).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith("SELECT 2"));
  });

  it("routes Form selection through the Rust relation proposal", async () => {
    const sql = "SELECT 1";
    const targetContext: SqlStructureAdapterContext = {
      ...context,
      parser: { ...context.parser, relation: "Orders" },
    };
    const relationFreeContext = buildSqlStructureContexts([], [], "new-sql")[0];
    const contexts = [targetContext, relationFreeContext];
    const onSqlChange = vi.fn();
    const proposedSql = 'SELECT * FROM "Orders"';
    analyzeMock.mockImplementation((
      _sql: string,
      parser: { relation: string },
    ) =>
      Promise.resolve(
        parser.relation
          ? {
            ...analysis,
            status: "unsupported",
            reason: "relation_unsupported",
          }
          : analysis,
      )
    );
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: proposedSql,
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => contexts}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "form")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "form")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Form" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Orders" }));

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      targetContext.parser,
      { kind: "set_relation", relation: "Orders" },
    );
    expect(onSqlChange).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith(proposedSql));
  });

  it("keeps directly authored reserved relations editable and hides them from the Form chooser", async () => {
    const forms: Form[] = [
      {
        id: "reserved-sql",
        name: "SQL",
        sql_relation: "form_reserved_sql",
        version: 1,
        template: "",
        fields: {},
      },
      {
        id: "user-orders",
        name: "Orders",
        sql_relation: "form_orders",
        version: 1,
        template: "",
        fields: {},
      },
    ];
    const contexts = buildSqlStructureContexts(forms, [], "new-sql");
    const sql = 'SELECT * FROM "form_reserved_sql"';
    const reservedContext = contexts.find((item) =>
      item.parser.relation === "form_reserved_sql"
    )!;
    analyzeMock.mockImplementation((
      _query: string,
      parser: { relation: string },
    ) =>
      Promise.resolve(
        parser.relation === "form_reserved_sql" ? analysis : {
          ...analysis,
          status: "unsupported",
          reason: "relation_unsupported",
        },
      )
    );
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => contexts}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    expect(analyzeMock).toHaveBeenCalledWith(sql, reservedContext.parser);
    actions.find((action) => action.kind === "columns")!.onActivate();
    const columnsDialog = screen.getByRole("dialog", { name: "Columns" });
    expect(within(columnsDialog).getByLabelText("Column 1")).toHaveValue(
      "_ugoite_id",
    );

    fireEvent.click(
      within(columnsDialog).getByRole("button", { name: "Close" }),
    );
    actions.find((action) => action.kind === "form")!.onActivate();
    const formDialog = screen.getByRole("dialog", { name: "Form" });
    expect(within(formDialog).getByRole("button", { name: "Orders" }))
      .toBeInTheDocument();
    expect(within(formDialog).queryByRole("button", { name: "SQL" }))
      .toBeNull();
  });

  it("waits for delayed parser initialization before allowing first Form selection", async () => {
    const sql = "SELECT 1";
    const targetContext: SqlStructureAdapterContext = {
      ...context,
      parser: { ...context.parser, relation: "Orders" },
    };
    const contexts = [targetContext];
    const onSqlChange = vi.fn();
    let resolveParser: (() => void) | undefined;
    prepareMock.mockImplementationOnce(() =>
      new Promise<void>((resolve) => resolveParser = resolve)
    );
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: 'SELECT * FROM "Orders"',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => contexts}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() => expect(prepareMock).toHaveBeenCalledOnce());
    expect(actions.find((action) => action.kind === "form")?.disabled)
      .toBe(true);
    expect(screen.queryByRole("dialog", { name: "Form" })).toBeNull();

    resolveParser?.();
    await waitFor(() =>
      expect(actions.find((action) => action.kind === "form")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "form")!.onActivate();
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "Form" })).getByRole(
        "button",
        { name: "Orders" },
      ),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(onSqlChange).not.toHaveBeenCalled();
  });

  it("discards a proposal and closes its dialog when SQL context changes", async () => {
    const initialSql = 'SELECT * FROM "Orders" ORDER BY "_ugoite_id"';
    const [sql, setSql] = createSignal(initialSql);
    const onSqlChange = vi.fn();
    let resolveProposal: ((result: SqlStructureProposal) => void) | undefined;
    proposeMock.mockImplementation(() =>
      new Promise((resolve) => {
        resolveProposal = resolve;
      })
    );
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    fireEvent.click(
      screen.getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());

    setSql('SELECT "amount" FROM "Orders"');
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    resolveProposal?.({
      contract_version: 5,
      status: "supported",
      base_sql: initialSql,
      proposed_sql: 'SELECT "amount" FROM "Orders"',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    await Promise.resolve();

    expect(onSqlChange).not.toHaveBeenCalled();
  });

  it("normalizes template variables only for parser proposals", async () => {
    const sql = 'SELECT * FROM "Orders" WHERE "amount" > {{threshold}}';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "columns")?.disabled)
        .toBe(false)
    );
    expect(analyzeMock).toHaveBeenCalledWith(
      'SELECT * FROM "Orders" WHERE "amount" > $threshold',
      context.parser,
    );
    actions.find((action) => action.kind === "columns")!.onActivate();
    fireEvent.click(
      screen.getByRole("button", { name: "Review change" }),
    );

    await waitFor(() =>
      expect(screen.getByText(sql, { selector: "pre" })).toBeInTheDocument()
    );
    expect(proposeMock).toHaveBeenCalledWith(
      'SELECT * FROM "Orders" WHERE "amount" > $threshold',
      context.parser,
      expect.objectContaining({ kind: "set_projection_slots" }),
    );
  });

  it("offers null filters without requiring a typed variable", async () => {
    const nullContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:null-operators",
      parser: { ...context.parser, variables: [] },
      columnCapabilities: {
        ...context.columnCapabilities,
        amount: {
          ...context.columnCapabilities.amount,
          supportedOperators: ["is_null", "is_not_null"],
        },
      },
    };
    const sql = 'SELECT * FROM "Orders"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [nullContext]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add filter" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      nullContext.parser,
      {
        kind: "set_filter_expression",
        expression: {
          kind: "predicate",
          column: "amount",
          operator: "is_null",
        },
      },
    );
  });

  it("uses the flat filter operation with older adapters", async () => {
    const legacyContext: SqlStructureAdapterContext = {
      ...context,
      parser: {
        ...context.parser,
        variables: [
          ...context.parser.variables,
          { name: "category_value", type: "string" },
        ],
      },
    };
    const legacyAnalysis: SqlStructureAnalysis = {
      ...joinedAnalysis,
      filters: [
        {
          column: "amount",
          qualifier: { name: "left_rows", quoted: true },
          operator: "gt",
          parameter: "threshold",
        },
        {
          column: "amount",
          qualifier: { name: "right_rows", quoted: true },
          operator: "eq",
          parameter: "threshold",
        },
      ],
      filter_expression: undefined,
      capabilities: {
        ...joinedAnalysis.capabilities,
        filter_expression: { enabled: false },
      },
    };
    proposeMock.mockImplementation(async (
      baseSql: string,
      _parser: unknown,
      operation: SqlStructureOperation,
    ) => {
      if (operation.kind === "set_filter_expression") {
        throw new Error("legacy adapter does not support filter trees");
      }
      return {
        contract_version: 1,
        status: "supported",
        base_sql: baseSql,
        proposed_sql: baseSql,
        requires_confirmation: true,
        added_stable_tie_breakers: [],
      } satisfies SqlStructureProposal;
    });
    analyzeMock.mockResolvedValue(legacyAnalysis);
    const sql =
      'SELECT * FROM "Orders" AS "left_rows" INNER JOIN "Orders" AS "right_rows" ON "left_rows"."category" = "right_rows"."category" WHERE "left_rows"."amount" > $threshold AND "right_rows"."amount" = $threshold';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [legacyContext]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    expect(
      within(dialog).getByRole("combobox", {
        name: "Filter group operator",
      }).querySelectorAll("option"),
    ).toHaveLength(1);
    for (
      const button of within(dialog).getAllByRole("button", {
        name: "Negate filter",
      })
    ) {
      expect(button).toBeDisabled();
    }
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      legacyContext.parser,
      {
        kind: "set_filters",
        filters: [
          {
            column: "amount",
            qualifier: { name: "left_rows", quoted: true },
            operator: "gt",
            parameter: "threshold",
          },
          {
            column: "amount",
            qualifier: { name: "right_rows", quoted: true },
            operator: "eq",
            parameter: "threshold",
          },
        ],
      },
    );
  });

  it("edits parsed AND, OR, and NOT filters as a nested tree", async () => {
    const treeContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:boolean-tree",
      columnCapabilities: {
        ...context.columnCapabilities,
        amount: {
          ...context.columnCapabilities.amount,
          supportedOperators: ["eq", "gt", "lt"],
        },
      },
    };
    const booleanAnalysis: SqlStructureAnalysis = {
      ...analysis,
      filters: [],
      filter_expression: {
        kind: "and",
        expressions: [
          {
            kind: "or",
            expressions: [
              {
                kind: "predicate",
                column: "amount",
                operator: "gt",
                parameter: "threshold",
              },
              {
                kind: "predicate",
                column: "amount",
                operator: "eq",
                parameter: "threshold",
              },
            ],
          },
          {
            kind: "not",
            expression: {
              kind: "predicate",
              column: "amount",
              operator: "lt",
              parameter: "threshold",
            },
          },
        ],
      },
    };
    analyzeMock.mockResolvedValue(booleanAnalysis);
    const sql =
      'SELECT * FROM "Orders" WHERE ("amount" > $threshold OR "amount" = $threshold) AND NOT ("amount" < $threshold)';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [treeContext]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    const groups = within(dialog).getAllByRole("group", {
      name: "Filter group",
    });
    expect(groups).toHaveLength(2);
    expect(within(dialog).getAllByText("NOT").length).toBeGreaterThan(0);
    fireEvent.change(
      within(groups[0]).getAllByRole("combobox", {
        name: "Filter group operator",
      })[0],
      { target: { value: "or" } },
    );
    const updatedGroups = within(dialog).getAllByRole("group", {
      name: "Filter group",
    });
    fireEvent.click(
      within(updatedGroups[1]).getByRole("button", { name: "Add filter" }),
    );
    const groupsAfterAddingFilter = within(dialog).getAllByRole("group", {
      name: "Filter group",
    });
    const firstNestedLeaf = groupsAfterAddingFilter[1].querySelector(
      ".sql-structure-filter-node",
    );
    expect(firstNestedLeaf).not.toBeNull();
    fireEvent.click(
      within(firstNestedLeaf as HTMLElement).getByRole("button", {
        name: "Negate filter",
      }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      treeContext.parser,
      {
        kind: "set_filter_expression",
        expression: {
          kind: "or",
          expressions: [
            {
              kind: "or",
              expressions: [
                {
                  kind: "not",
                  expression: {
                    kind: "predicate",
                    column: "amount",
                    operator: "gt",
                    parameter: "threshold",
                  },
                },
                {
                  kind: "predicate",
                  column: "amount",
                  operator: "eq",
                  parameter: "threshold",
                },
                {
                  kind: "predicate",
                  column: "amount",
                  operator: "eq",
                  parameter: "threshold",
                },
              ],
            },
            {
              kind: "not",
              expression: {
                kind: "predicate",
                column: "amount",
                operator: "lt",
                parameter: "threshold",
              },
            },
          ],
        },
      },
    );
  });

  it("keeps keyboard focus on a filter control after editing the tree", async () => {
    const treeContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:filter-focus",
      parser: {
        ...context.parser,
        variables: [
          ...context.parser.variables,
          { name: "category_value", type: "string" },
        ],
      },
    };
    analyzeMock.mockResolvedValue({
      ...analysis,
      filter_expression: {
        kind: "and",
        expressions: [
          {
            kind: "predicate",
            column: "amount",
            operator: "gt",
            parameter: "threshold",
          },
          {
            kind: "predicate",
            column: "category",
            operator: "eq",
            parameter: "category_value",
          },
        ],
      },
    });
    const sql =
      'SELECT * FROM "Orders" WHERE "amount" > $threshold AND "category" = $category_value';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [treeContext]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    const operator = within(dialog).getByRole("combobox", {
      name: "Filter group operator",
    });
    operator.focus();
    fireEvent.change(operator, { target: { value: "or" } });

    await waitFor(() =>
      expect(
        within(dialog).getByRole("combobox", {
          name: "Filter group operator",
        }),
      ).toHaveFocus()
    );
  });

  it("creates a typed argument when filtering a fresh Form query", async () => {
    const emptyContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:no-variables",
      parser: { ...context.parser, variables: [] },
      columnCapabilities: {
        ...context.columnCapabilities,
        _ugoite_id: {
          ...context.columnCapabilities._ugoite_id,
          filterable: false,
          supportedOperators: [],
        },
      },
    };
    const sql = 'SELECT * FROM "Orders"';
    let currentSql = sql;
    let declaredVariables: {
      name: string;
      type: SqlStructureVariableType;
    }[] = [];
    const onSqlChange = vi.fn((nextSql: string) => {
      currentSql = nextSql;
    });
    const onVariablesAdded = vi.fn((
      added: readonly {
        name: string;
        type: string;
      }[],
    ) => {
      declaredVariables = [...declaredVariables, ...added];
    });
    const onVariablesRemoved = vi.fn((names: readonly string[]) => {
      const removed = new Set(names);
      declaredVariables = declaredVariables.filter(({ name }) =>
        !removed.has(name)
      );
    });
    proposeMock.mockResolvedValue(
      {
        contract_version: 5,
        status: "supported",
        base_sql: sql,
        proposed_sql: 'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
        requires_confirmation: true,
        added_stable_tie_breakers: [],
      } satisfies SqlStructureProposal,
    );
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => currentSql}
        contexts={() => [emptyContext]}
        onSqlChange={onSqlChange}
        onVariablesAdded={onVariablesAdded}
        onVariablesRemoved={onVariablesRemoved}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add filter" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      {
        ...emptyContext.parser,
        variables: [{ name: "filter_amount", type: "integer" }],
      },
      {
        kind: "set_filter_expression",
        expression: {
          kind: "predicate",
          column: "amount",
          operator: "eq",
          parameter: "filter_amount",
        },
      },
    );

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => {
      expect(onVariablesAdded).toHaveBeenCalledWith([{
        name: "filter_amount",
        type: "integer",
      }]);
      expect(onSqlChange).toHaveBeenCalledWith(
        'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
      );
      expect(declaredVariables).toEqual([{
        name: "filter_amount",
        type: "integer",
      }]);
    });

    actions.find((action) => action.kind === "filter")!.onActivate();
    const historyDialog = screen.getByRole("dialog", { name: "Filter" });
    const undo = within(historyDialog).getByRole("button", {
      name: "Undo structure edit",
    });
    expect(undo).toBeEnabled();
    fireEvent.click(undo);
    await waitFor(() => expect(currentSql).toBe(sql));
    expect(declaredVariables).toEqual([]);
    expect(onVariablesRemoved).toHaveBeenCalledWith(["filter_amount"]);

    const redo = within(historyDialog).getByRole("button", {
      name: "Redo structure edit",
    });
    expect(redo).toBeEnabled();
    fireEvent.click(redo);
    await waitFor(() =>
      expect(currentSql).toBe(
        'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
      )
    );
    expect(declaredVariables).toEqual([{
      name: "filter_amount",
      type: "integer",
    }]);
  });

  it("keeps SQL unchanged when the host rejects a typed filter argument", async () => {
    const emptyContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:rejected-variable",
      parser: { ...context.parser, variables: [] },
      columnCapabilities: {
        ...context.columnCapabilities,
        _ugoite_id: {
          ...context.columnCapabilities._ugoite_id,
          filterable: false,
          supportedOperators: [],
        },
      },
    };
    let currentSql = 'SELECT * FROM "Orders"';
    const onSqlChange = vi.fn((sql: string) => {
      currentSql = sql;
    });
    const onVariablesAdded = vi.fn(() => false);
    proposeMock.mockResolvedValue(
      {
        contract_version: 5,
        status: "supported",
        base_sql: currentSql,
        proposed_sql: 'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
        requires_confirmation: true,
        added_stable_tie_breakers: [],
      } satisfies SqlStructureProposal,
    );
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => currentSql}
        contexts={() => [emptyContext]}
        onSqlChange={onSqlChange}
        onVariablesAdded={onVariablesAdded}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add filter" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    const applyButton = await within(dialog).findByRole("button", {
      name: "Apply change",
    });
    fireEvent.click(applyButton);

    await waitFor(() =>
      expect(within(dialog).getByRole("alert")).toBeInTheDocument()
    );
    expect(onVariablesAdded).toHaveBeenCalledWith([{
      name: "filter_amount",
      type: "integer",
    }]);
    expect(onSqlChange).not.toHaveBeenCalled();
    expect(currentSql).toBe('SELECT * FROM "Orders"');
    expect(within(dialog).getByRole("alert")).toBeInTheDocument();
  });

  it("avoids variable IDs already owned by the host", async () => {
    const emptyContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:reserved-variable",
      parser: { ...context.parser, variables: [] },
      columnCapabilities: {
        ...context.columnCapabilities,
        _ugoite_id: {
          ...context.columnCapabilities._ugoite_id,
          filterable: false,
          supportedOperators: [],
        },
      },
    };
    const sql = 'SELECT * FROM "Orders"';
    const onSqlChange = vi.fn();
    const onVariablesAdded = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    proposeMock.mockResolvedValue(
      {
        contract_version: 5,
        status: "supported",
        base_sql: sql,
        proposed_sql:
          'SELECT * FROM "Orders" WHERE "amount" = $filter_amount_2',
        requires_confirmation: true,
        added_stable_tie_breakers: [],
      } satisfies SqlStructureProposal,
    );

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [emptyContext]}
        onSqlChange={onSqlChange}
        onVariablesAdded={onVariablesAdded}
        reservedVariableNames={() => ["filter_amount"]}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add filter" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      {
        ...emptyContext.parser,
        variables: [{ name: "filter_amount_2", type: "integer" }],
      },
      {
        kind: "set_filter_expression",
        expression: {
          kind: "predicate",
          column: "amount",
          operator: "eq",
          parameter: "filter_amount_2",
        },
      },
    );

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => {
      expect(onVariablesAdded).toHaveBeenCalledWith([{
        name: "filter_amount_2",
        type: "integer",
      }]);
      expect(onSqlChange).toHaveBeenCalledWith(
        'SELECT * FROM "Orders" WHERE "amount" = $filter_amount_2',
      );
    });
  });

  it("locks filter drafts while a typed argument proposal is pending", async () => {
    const emptyContext: SqlStructureAdapterContext = {
      ...context,
      identity: "inline:source-1:form-1:pending-filter-proposal",
      parser: { ...context.parser, variables: [] },
      columnCapabilities: {
        ...context.columnCapabilities,
        _ugoite_id: {
          ...context.columnCapabilities._ugoite_id,
          filterable: false,
          supportedOperators: [],
        },
      },
    };
    const sql = 'SELECT * FROM "Orders"';
    const onSqlChange = vi.fn();
    const onVariablesAdded = vi.fn();
    let resolveProposal: (value: SqlStructureProposal) => void = () => {};
    proposeMock.mockImplementationOnce(() =>
      new Promise<SqlStructureProposal>((resolve) => {
        resolveProposal = resolve;
      })
    );
    let actions: readonly SQLWorkspaceAction[] = [];

    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [emptyContext]}
        onSqlChange={onSqlChange}
        onVariablesAdded={onVariablesAdded}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "filter")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "filter")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Filter" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add filter" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(within(dialog).getByLabelText("Column")).toBeDisabled();
    expect(within(dialog).getByLabelText("Operator")).toBeDisabled();
    expect(within(dialog).getByLabelText("Variable")).toBeDisabled();
    expect(
      within(dialog).getByRole("button", { name: "Remove filter" }),
    ).toBeDisabled();
    expect(
      within(dialog).getByRole("button", { name: "Add filter" }),
    ).toBeDisabled();

    resolveProposal({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: 'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "Apply change" }),
    );

    await waitFor(() => expect(onVariablesAdded).toHaveBeenCalledOnce());
    expect(onVariablesAdded).toHaveBeenCalledWith([{
      name: "filter_amount",
      type: "integer",
    }]);
    await waitFor(() =>
      expect(onSqlChange).toHaveBeenCalledWith(
        'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
      )
    );
  });

  it("proposes a type-gated SUM DISTINCT and applies only after confirmation", async () => {
    const sql = 'SELECT * FROM "Orders"';
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "aggregate")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "aggregate")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Aggregate" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate" }),
    );
    fireEvent.change(within(dialog).getByLabelText("Function"), {
      target: { value: "sum" },
    });
    const distinct = within(dialog).getByLabelText("Distinct values");
    expect(distinct).toBeEnabled();
    fireEvent.click(distinct);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      {
        kind: "set_aggregates",
        aggregates: [{
          function: "sum",
          column: "amount",
          distinct: true,
        }],
      },
    );
    expect(onSqlChange).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(onSqlChange).toHaveBeenCalledOnce());
  });

  it("keeps COUNT(*) available while allowing COUNT(DISTINCT column)", async () => {
    const sql = 'SELECT * FROM "Orders"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "aggregate")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "aggregate")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Aggregate" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate" }),
    );

    const column = () => within(dialog).getByLabelText("Column");
    expect(column()).toHaveValue("");
    expect(column().closest(".sql-structure-aggregate-row")).toHaveClass(
      "sql-structure-aggregate-row--count-all",
    );
    expect(
      within(column()).getByRole("option", { name: "All rows (*)" }),
    ).toBeInTheDocument();
    fireEvent.change(column(), { target: { value: "amount" } });
    await waitFor(() => expect(column()).toHaveValue("amount"));
    expect(
      within(column()).getByRole("option", { name: "All rows (*)" }),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByLabelText("Distinct values"));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      {
        kind: "set_aggregates",
        aggregates: [{
          function: "count",
          column: "amount",
          distinct: true,
        }],
      },
    );
  });

  it("edits COUNT(DISTINCT *) and multi-column COUNT arguments", async () => {
    const sql = 'SELECT * FROM "Orders"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "aggregate")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "aggregate")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Aggregate" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate" }),
    );
    fireEvent.click(within(dialog).getByLabelText("Distinct values"));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(sql, context.parser, {
      kind: "set_aggregates",
      aggregates: [{
        function: "count",
        distinct: true,
      }],
    });
  });

  it("adds multiple COUNT arguments from the aggregate editor", async () => {
    const sql = 'SELECT * FROM "Orders"';
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "aggregate")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "aggregate")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Aggregate" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate argument" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate argument" }),
    );
    expect(within(dialog).getAllByLabelText("Column")).toHaveLength(2);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    const operation = proposeMock.mock.calls[0][2] as {
      aggregates: Array<{
        column?: string;
        arguments?: string[];
        distinct: boolean;
      }>;
    };
    expect(operation.aggregates[0].column).toBeTruthy();
    expect(operation.aggregates[0].arguments).toHaveLength(1);
    expect(operation.aggregates[0].distinct).toBe(false);
  });

  it("proposes only capability-approved GROUP BY keys and SELECT DISTINCT", async () => {
    const sql = 'SELECT "amount" FROM "Orders"';
    analyzeMock.mockResolvedValue({ ...analysis, projection: ["amount"] });
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "group_by")?.disabled)
        .toBe(false)
    );
    expect(actions.find((action) => action.kind === "group_by")?.icon).toBe(
      "group-by",
    );
    actions.find((action) => action.kind === "group_by")!.onActivate();
    const groupDialog = screen.getByRole("dialog", { name: "Group by" });
    const amountKey = within(groupDialog).getByLabelText("amount");
    expect(within(groupDialog).queryByLabelText("private_note")).toBeNull();
    fireEvent.click(amountKey);
    fireEvent.click(
      within(groupDialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      { kind: "set_group_by", columns: ["amount"] },
    );

    fireEvent.click(
      within(groupDialog).getByRole("button", { name: "Cancel" }),
    );
    actions.find((action) => action.kind === "distinct")!.onActivate();
    const distinctDialog = screen.getByRole("dialog", {
      name: "Distinct",
    });
    fireEvent.click(
      within(distinctDialog).getByLabelText("Select distinct rows"),
    );
    fireEvent.click(
      within(distinctDialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledTimes(2));
    expect(proposeMock).toHaveBeenLastCalledWith(
      sql,
      context.parser,
      { kind: "set_distinct", distinct: true },
    );
  });

  it("lets a wildcard query choose group keys and applies only the confirmed Rust proposal", async () => {
    const sql =
      '-- report\nSELECT * FROM "Orders" WHERE "amount" > $threshold ORDER BY "amount" DESC';
    const proposedSql =
      '-- report\nSELECT "amount" FROM "Orders" WHERE "amount" > $threshold GROUP BY "amount" ORDER BY "amount" DESC';
    analyzeMock.mockResolvedValue({ ...analysis, projection: ["*"] });
    proposeMock.mockResolvedValue({
      contract_version: 5,
      status: "supported",
      base_sql: sql,
      proposed_sql: proposedSql,
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    const onSqlChange = vi.fn();
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => sql}
        contexts={() => [context]}
        onSqlChange={onSqlChange}
        onActionsChange={(next) => actions = next}
      />
    ));

    await waitFor(() =>
      expect(actions.find((action) => action.kind === "group_by")?.disabled)
        .toBe(false)
    );
    actions.find((action) => action.kind === "group_by")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Group by" });
    fireEvent.click(within(dialog).getByLabelText("amount"));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );

    await waitFor(() =>
      expect(within(dialog).getByText("Proposed SQL")).toBeInTheDocument()
    );
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      context.parser,
      { kind: "set_group_by", columns: ["amount"] },
    );
    const proposedHeading = within(dialog).getByRole("heading", {
      name: "Proposed SQL",
    });
    expect(proposedHeading.nextElementSibling?.textContent).toBe(proposedSql);
    expect(onSqlChange).not.toHaveBeenCalled();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    await waitFor(() => expect(onSqlChange).toHaveBeenCalledWith(proposedSql));
  });

  it("falls back to SQL structure without trusted type metadata", async () => {
    const untrustedContext: SqlStructureAdapterContext = {
      ...context,
      columnCapabilities: Object.fromEntries(
        Object.entries(context.columnCapabilities).map(([name, value]) => [
          name,
          {
            ...value,
            aggregateFunctions: [],
            groupable: false,
            distinctable: false,
          },
        ]),
      ),
    };
    analyzeMock.mockResolvedValue({ ...analysis, projection: ["amount"] });
    let actions: readonly SQLWorkspaceAction[] = [];
    render(() => (
      <SqlStructureToolbar
        sql={() => 'SELECT "amount" FROM "Orders"'}
        contexts={() => [untrustedContext]}
        onSqlChange={vi.fn()}
        onActionsChange={(next) => actions = next}
      />
    ));
    await waitFor(() =>
      expect(actions.find((action) => action.kind === "aggregate")?.disabled)
        .toBe(false)
    );
    expect(actions).toHaveLength(8);
    for (const kind of ["group_by", "distinct"] as const) {
      expect(actions.find((action) => action.kind === kind)?.disabled).toBe(
        false,
      );
    }
    actions.find((action) => action.kind === "group_by")!.onActivate();
    expect(
      screen.getByRole("dialog", { name: "Group by" }).querySelector(
        "nav[aria-label='SQL structure actions']",
      ),
    ).toBeInTheDocument();
    actions.find((action) => action.kind === "aggregate")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Aggregate" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate" }),
    );
    expect(within(dialog).getByLabelText("Function")).toHaveValue("count");
    expect(within(dialog).queryByRole("option", { name: "SUM" })).toBeNull();
  });
});
