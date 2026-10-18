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

const { analyzeMock, proposeMock } = vi.hoisted(() => ({
  analyzeMock: vi.fn(),
  proposeMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  analyzeSqlStructure: analyzeMock,
  prepareSqlStructureParser: vi.fn(async () => undefined),
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
  contract_version: 2,
  status: "supported",
  projection: ["*"],
  filters: [],
  aggregates: [],
  group_by: [],
  distinct: false,
  order_by: [{ column: "_ugoite_id", direction: "asc" }],
  referenced_variables: [],
  stable_pagination: true,
  capabilities: {
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
    proposeMock.mockReset();
    analyzeMock.mockResolvedValue(analysis);
    proposeMock.mockImplementation(async (baseSql: string) => ({
      contract_version: 2,
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

    await waitFor(() => expect(actions).toHaveLength(6));
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
      referenced_variables: [],
      stable_pagination: false,
      capabilities: {
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
      contract_version: 2,
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
          arguments: [{ kind: "column", column: "amount" }],
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
    expect(column()).toHaveValue("*");
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
          arguments: [{ kind: "column", column: "amount" }],
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
        arguments: [{ kind: "wildcard" }],
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
    expect(within(dialog).queryByRole("option", { name: "All rows" }))
      .toBeNull();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Review change" }),
    );
    await waitFor(() => expect(proposeMock).toHaveBeenCalledOnce());
    const operation = proposeMock.mock.calls[0][2] as {
      aggregates: Array<{ arguments: unknown[]; distinct: boolean }>;
    };
    expect(operation.aggregates[0].arguments).toHaveLength(2);
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
    expect(actions).toHaveLength(6);
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
