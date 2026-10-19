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
import type { SQLWorkspaceAction } from "./SQLWorkspace";
import type {
  SqlStructureAnalysis,
  SqlStructureProposal,
} from "~/lib/ugoite-client";
import { SqlStructureToolbar } from "./SqlStructureToolbar";

const { analyzeMock, prepareMock, proposeMock } = vi.hoisted(() => ({
  analyzeMock: vi.fn(),
  prepareMock: vi.fn(),
  proposeMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  analyzeSqlStructure: analyzeMock,
  prepareSqlStructureParser: prepareMock,
  proposeSqlStructureOperation: proposeMock,
}));

const context: SqlStructureAdapterContext = {
  identity: "inline:source-1:form-1:3",
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
      aggregateFunctions: ["count", "sum", "avg", "min", "max"],
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
  contract_version: 1,
  status: "supported",
  projection: ["*"],
  filters: [],
  aggregates: [],
  group_by: [],
  distinct: false,
  order_by: [{ column: "_ugoite_id", direction: "asc" }],
  joins: [],
  referenced_variables: [],
  stable_pagination: true,
  capabilities: {
    join: { enabled: true },
    columns: { enabled: true },
    filter: { enabled: true },
    sort: { enabled: true },
    aggregate: { enabled: true },
    group_by: { enabled: true },
    distinct: { enabled: true },
    stable_pagination: { enabled: true },
  },
};

describe("SqlStructureToolbar adapter", () => {
  beforeEach(() => {
    setLocale("en");
    analyzeMock.mockReset();
    prepareMock.mockReset();
    proposeMock.mockReset();
    prepareMock.mockResolvedValue(undefined);
    analyzeMock.mockResolvedValue(analysis);
    proposeMock.mockImplementation(async (baseSql: string) => ({
      contract_version: 1,
      status: "supported",
      base_sql: baseSql,
      proposed_sql: 'SELECT "amount" FROM "Orders" ORDER BY "_ugoite_id" ASC',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    }));
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
      .toBe(true);
    expect(actions.find((action) => action.kind === "distinct")?.disabled)
      .toBe(true);

    actions.find((action) => action.kind === "columns")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Columns" });
    const privateColumn = within(dialog).getByLabelText("private_note");
    expect(privateColumn).toBeDisabled();
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
    expect(onSqlChange).toHaveBeenCalledWith(
      'SELECT "amount" FROM "Orders" ORDER BY "_ugoite_id" ASC',
    );
  });

  it("adds a typed inner self-join through the shared toolbar", async () => {
    const sql = 'SELECT * FROM "Orders"';
    const proposedSql =
      'SELECT * FROM "Orders" AS "_ugoite_left" INNER JOIN "Orders" AS "_ugoite_right" ON "_ugoite_left"."amount" = "_ugoite_right"."amount"';
    proposeMock.mockResolvedValue({
      contract_version: 1,
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
    const dialog = screen.getByRole("dialog", { name: "Join" });
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
    expect(onSqlChange).toHaveBeenCalledWith(proposedSql);
  });

  it("offers a safe structure edit for scalar SELECT without a Form", async () => {
    const sql = "SELECT 1";
    const contexts = buildSqlStructureContexts([], [], "new-sql");
    const scalarAnalysis: SqlStructureAnalysis = {
      contract_version: 1,
      status: "supported",
      projection: ["1"],
      filters: [],
      aggregates: [],
      group_by: [],
      distinct: false,
      order_by: [],
      joins: [],
      referenced_variables: [],
      stable_pagination: false,
      capabilities: {
        join: { enabled: false, reason: "invalid_context" },
        columns: { enabled: false, reason: "invalid_context" },
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
    expect(onSqlChange).toHaveBeenCalledWith("SELECT DISTINCT 1");
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
      contract_version: 1,
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
    expect(onSqlChange).toHaveBeenCalledWith(proposedSql);
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
      contract_version: 1,
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
      contract_version: 1,
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
      expect.objectContaining({ kind: "set_columns" }),
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
        kind: "set_filters",
        filters: [{ column: "amount", operator: "is_null" }],
      },
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
    const onSqlChange = vi.fn();
    const onVariablesAdded = vi.fn();
    proposeMock.mockResolvedValue(
      {
        contract_version: 1,
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
    expect(proposeMock).toHaveBeenCalledWith(
      sql,
      {
        ...emptyContext.parser,
        variables: [{ name: "filter_amount", type: "integer" }],
      },
      {
        kind: "set_filters",
        filters: [{
          column: "amount",
          operator: "eq",
          parameter: "filter_amount",
        }],
      },
    );

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply change" }),
    );
    expect(onVariablesAdded).toHaveBeenCalledWith([{
      name: "filter_amount",
      type: "integer",
    }]);
    expect(onSqlChange).toHaveBeenCalledWith(
      'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
    );
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
      contract_version: 1,
      status: "supported",
      base_sql: sql,
      proposed_sql: 'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
      requires_confirmation: true,
      added_stable_tie_breakers: [],
    });
    fireEvent.click(
      await within(dialog).findByRole("button", { name: "Apply change" }),
    );

    expect(onVariablesAdded).toHaveBeenCalledWith([{
      name: "filter_amount",
      type: "integer",
    }]);
    expect(onSqlChange).toHaveBeenCalledWith(
      'SELECT * FROM "Orders" WHERE "amount" = $filter_amount',
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
    expect(onSqlChange).toHaveBeenCalledOnce();
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

  it("keeps typed actions gated without trusted type metadata", async () => {
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
        true,
      );
    }
    actions.find((action) => action.kind === "aggregate")!.onActivate();
    const dialog = screen.getByRole("dialog", { name: "Aggregate" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add aggregate" }),
    );
    expect(within(dialog).getByLabelText("Function")).toHaveValue("count");
    expect(within(dialog).queryByRole("option", { name: "SUM" })).toBeNull();
  });
});
