import "@testing-library/jest-dom/vitest";
import { createSignal, Show } from "solid-js";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import * as client from "~/lib/ugoite-client";
import { describe, expect, it, vi } from "vitest";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import { SqlFilterSortEditor } from "./SqlFilterSortEditor";
import type { SqlConditionWorkspaceHistoryEntry } from "./SqlConditionWorkspace";

const context: SqlStructureAdapterContext = {
  identity: "expenses",
  relationSelectable: true,
  label: "Expenses",
  parser: {
    relation: "form_expenses",
    columns: [{ name: "field_amount", type: "integer" }],
    variables: [],
    stable_key: [],
  },
  columnCapabilities: {
    field_amount: {
      projectable: true,
      filterable: true,
      sortable: true,
      supportedOperators: ["eq", "gt_eq", "is_null", "is_not_null"],
      aggregateFunctions: [],
      groupable: true,
      distinctable: true,
    },
  },
  columnLabels: { field_amount: "Amount" },
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => resolve = complete);
  return { promise, resolve };
}

describe("SQL Filter and Sort task editors", () => {
  it("switches reactively between filter and sort editors", async () => {
    const [action, setAction] = createSignal<"filter" | "sort">("sort");
    render(() => (
      <SqlFilterSortEditor
        action={action()}
        sql={() => 'SELECT * FROM "form_expenses"'}
        scope={() => "composition-query:query-a"}
        contexts={() => [context]}
        onSqlChange={() => {}}
        onAdvanced={() => {}}
      />
    ));
    expect(document.querySelector(".sql-condition-workspace")).toBeNull();
    setAction("filter");
    await waitFor(() =>
      expect(document.querySelector(".sql-condition-workspace")).not.toBeNull()
    );
    setAction("sort");
    await waitFor(() =>
      expect(document.querySelector(".sql-condition-workspace")).toBeNull()
    );
  });

  it("constructs a named filter argument and explicit binding in the host commit", async () => {
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    const [bindings, setBindings] = createSignal<
      readonly { name: string; variableId: string }[]
    >([]);
    const [argumentsList, setArgumentsList] = createSignal<
      readonly { name: string; type: string; description: string }[]
    >([]);
    const onFilterEdit = vi.fn((change: {
      resultSql: string;
      expectedSql: string;
      createdArgument?: { name: string; type: string };
      binding?: { name: string; variableId: string };
      expectedBindings?: readonly { name: string; variableId: string }[];
      resultBindings?: readonly { name: string; variableId: string }[];
      expectedArguments?: readonly {
        name: string;
        type: string;
        description: string;
      }[];
      resultArguments?: readonly {
        name: string;
        type: string;
        description: string;
      }[];
    }) => {
      if (change.expectedSql !== sql()) return false;
      if (
        JSON.stringify(change.expectedBindings ?? bindings()) !==
          JSON.stringify(bindings())
      ) return false;
      if (
        JSON.stringify(change.expectedArguments ?? argumentsList()) !==
          JSON.stringify(argumentsList())
      ) return false;
      setSql(change.resultSql);
      if (change.resultBindings) setBindings(change.resultBindings);
      if (change.resultArguments) setArgumentsList(change.resultArguments);
      return true;
    });
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        scope={() => "composition-query:query-a"}
        contexts={() => [context]}
        argumentDefinitions={argumentsList}
        scalarVariables={[{
          id: "min_amount",
          name: "Minimum amount",
          type: "integer",
        }]}
        argumentBindings={bindings}
        onFilterEdit={onFilterEdit}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const filterColumn = await screen.findByRole("combobox", {
      name: "Column for new filter",
    });
    await waitFor(() =>
      expect(filterColumn.querySelectorAll("option").length).toBe(2)
    );
    fireEvent.change(filterColumn, { target: { value: "field_amount" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Value source" }), {
      target: { value: "argument" },
    });
    expect(screen.queryByRole("combobox", { name: "Value type" })).toBeNull();
    fireEvent.input(
      screen.getByRole("textbox", { name: "New argument name" }),
      {
        target: { value: "minimum" },
      },
    );
    fireEvent.change(
      screen.getByRole("combobox", { name: "Bind to scalar variable" }),
      {
        target: { value: "min_amount" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    await waitFor(() => expect(onFilterEdit).toHaveBeenCalled());
    expect(onFilterEdit.mock.calls[0][0]).toMatchObject({
      expectedSql: 'SELECT * FROM "form_expenses"',
      expectedScope: "composition-query:query-a",
      createdArgument: { name: "minimum", type: "integer" },
      binding: { name: "minimum", variableId: "min_amount" },
      expectedBindings: [],
      resultBindings: [{ name: "minimum", variableId: "min_amount" }],
    });
    expect(sql()).toContain("field_amount = $minimum");
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(sql()).toBe('SELECT * FROM "form_expenses"'));
    expect(onFilterEdit).toHaveBeenCalledTimes(2);
    expect(onFilterEdit.mock.calls[1][0]).toMatchObject({
      expectedBindings: [{ name: "minimum", variableId: "min_amount" }],
      resultBindings: [],
    });
    setArgumentsList([{ name: "later", type: "integer", description: "" }]);
    fireEvent.click(screen.getByRole("button", { name: "Redo" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
    expect(sql()).toBe('SELECT * FROM "form_expenses"');
    expect(argumentsList()).toEqual([
      { name: "later", type: "integer", description: "" },
    ]);
  });

  it("creates standalone arguments with the selected column type and default, without creating one for null operators", async () => {
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    const onArgumentAdded = vi.fn();
    const standalone = render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [context]}
        argumentDefinitions={() => []}
        onArgumentAdded={onArgumentAdded}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const column = await screen.findByRole("combobox", {
      name: "Column for new filter",
    });
    await waitFor(() =>
      expect(column.querySelectorAll("option").length).toBe(2)
    );
    fireEvent.change(column, { target: { value: "field_amount" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Value source" }), {
      target: { value: "argument" },
    });
    fireEvent.input(
      screen.getByRole("textbox", { name: "New argument name" }),
      {
        target: { value: "minimum" },
      },
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Default value (optional)" }),
      {
        target: { value: "12" },
      },
    );
    expect(screen.getByText("integer")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    await waitFor(() =>
      expect(onArgumentAdded).toHaveBeenCalledWith({
        name: "minimum",
        type: "integer",
        description: "",
        default: 12,
      })
    );
    await waitFor(() => expect(sql()).toContain("$minimum"));
    standalone.unmount();

    const [nullSql, setNullSql] = createSignal('SELECT * FROM "form_expenses"');
    const nullAdded = vi.fn();
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={nullSql}
        contexts={() => [context]}
        argumentDefinitions={() => []}
        onArgumentAdded={nullAdded}
        onSqlChange={setNullSql}
        onAdvanced={() => {}}
      />
    ));
    const nullColumn = await screen.findByRole("combobox", {
      name: "Column for new filter",
    });
    await waitFor(() =>
      expect(nullColumn.querySelectorAll("option").length).toBe(2)
    );
    fireEvent.change(nullColumn, {
      target: { value: "field_amount" },
    });
    fireEvent.change(
      screen.getByRole("combobox", { name: "Operator for new filter" }),
      {
        target: { value: "is_null" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    await waitFor(() => expect(nullSql()).toContain("IS NULL"));
    expect(nullAdded).not.toHaveBeenCalled();
  });

  it("keeps a typed filter draft stable while typing and applies it through Rust", async () => {
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [context]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const filterColumn = await screen.findByRole("combobox", {
      name: "Column for new filter",
    });
    await waitFor(() =>
      expect(filterColumn.querySelectorAll("option").length).toBe(2)
    );
    fireEvent.change(filterColumn, {
      target: { value: "field_amount" },
    });
    fireEvent.change(
      screen.getByRole("combobox", { name: "Operator for new filter" }),
      {
        target: { value: "gte" },
      },
    );
    const value = screen.getByRole("textbox", { name: "Value for new filter" });
    fireEvent.input(value, { target: { value: "9007199254740993" } });
    expect(screen.getByRole("textbox", { name: "Value for new filter" })).toBe(
      value,
    );
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    await waitFor(() =>
      expect(sql()).toBe(
        'SELECT * FROM "form_expenses" WHERE field_amount >= 9007199254740993',
      )
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(sql()).toBe('SELECT * FROM "form_expenses"'));
    fireEvent.click(screen.getByRole("button", { name: "Redo" }));
    await waitFor(() =>
      expect(sql()).toBe(
        'SELECT * FROM "form_expenses" WHERE field_amount >= 9007199254740993',
      )
    );
  });

  it("shows editable existing conditions and preserves every argument on a leaf edit", async () => {
    const [sql, setSql] = createSignal(
      'SELECT * FROM "form_expenses" WHERE field_amount >= $minimum AND field_amount <= $maximum',
    );
    const [argumentsList, setArgumentsList] = createSignal<
      readonly { name: string; type: string; description: string }[]
    >([
      { name: "minimum", type: "integer", description: "lower bound" },
      { name: "maximum", type: "integer", description: "upper bound" },
      { name: "unused", type: "string", description: "keep this declaration" },
    ]);
    const [bindings, setBindings] = createSignal<
      readonly { name: string; variableId: string }[]
    >([]);
    const edit = vi.fn((change: {
      expectedSql: string;
      expectedArguments?: readonly {
        name: string;
        type: string;
        description: string;
      }[];
      resultArguments?: readonly {
        name: string;
        type: string;
        description: string;
      }[];
      resultSql: string;
      expectedBindings?: readonly { name: string; variableId: string }[];
      resultBindings?: readonly { name: string; variableId: string }[];
    }) => {
      if (change.expectedSql !== sql()) return false;
      if (
        JSON.stringify(change.expectedArguments ?? argumentsList()) !==
          JSON.stringify(argumentsList())
      ) return false;
      if (
        JSON.stringify(change.expectedBindings ?? bindings()) !==
          JSON.stringify(bindings())
      ) return false;
      setSql(change.resultSql);
      if (change.resultArguments) setArgumentsList(change.resultArguments);
      if (change.resultBindings) setBindings(change.resultBindings);
      return true;
    });
    const editableContext: SqlStructureAdapterContext = {
      ...context,
      columnCapabilities: {
        ...context.columnCapabilities,
        field_amount: {
          ...context.columnCapabilities.field_amount!,
          supportedOperators: [
            "eq",
            "gt",
            "gt_eq",
            "lt_eq",
            "is_null",
            "is_not_null",
          ],
        },
      },
    };
    const [contexts, setContexts] = createSignal([editableContext]);
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        scope={() => "saved-query:query-a"}
        contexts={contexts}
        argumentDefinitions={argumentsList}
        argumentBindings={bindings}
        onFilterEdit={edit}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const rows = await screen.findAllByRole("group", { name: "Amount" });
    expect(rows).toHaveLength(2);
    fireEvent.click(
      within(rows[0]).getByRole("button", { name: "Edit condition" }),
    );
    const operator = within(rows[0]).getByRole("combobox", {
      name: "Operator",
    });
    fireEvent.change(operator, { target: { value: "gt" } });
    expect(operator).toHaveValue("gt");
    fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    await waitFor(() => expect(edit).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(sql()).toContain("field_amount > $minimum"));
    expect(sql()).toContain("field_amount <= $maximum");
    expect(argumentsList()).toEqual([
      { name: "minimum", type: "integer", description: "lower bound" },
      { name: "maximum", type: "integer", description: "upper bound" },
      { name: "unused", type: "string", description: "keep this declaration" },
    ]);
    expect(edit.mock.calls[0][0].resultArguments).toEqual(argumentsList());
    fireEvent.click(
      screen.getAllByRole("button", { name: "Add condition" }).at(-1)!,
    );
    setContexts([structuredClone(editableContext)]);
    setArgumentsList(argumentsList().map((argument) => ({ ...argument })));
    await waitFor(() => {
      const refreshedRows = screen.getAllByRole("group", { name: "Amount" });
      expect(
        within(refreshedRows[0]).getByRole("combobox", { name: "Column" }),
      ).toHaveValue("field_amount");
      expect(
        within(refreshedRows[0]).getByRole("combobox", {
          name: "Declared argument",
        }),
      ).toHaveValue("minimum");
      expect(
        screen.getByRole("combobox", { name: "Column for new filter" }),
      ).toHaveValue("");
    });
    await waitFor(() =>
      expect(document.activeElement).toBe(
        document.querySelector('[data-condition-focus="7:operator"]'),
      )
    );
  });

  it("discards an existing-condition draft when SQL changes externally", async () => {
    const [sql, setSql] = createSignal(
      'SELECT * FROM "form_expenses" WHERE field_amount >= 4',
    );
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [context]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    fireEvent.click(
      await screen.findAllByRole("button", { name: "Edit condition" }).then(
        (buttons) => buttons[0],
      ),
    );
    const value = await screen.findByRole("textbox", { name: "Value" });
    expect(value).toHaveValue("4");
    fireEvent.input(value, { target: { value: "99" } });
    expect(screen.getByRole("button", { name: "Apply changes" }))
      .toBeInTheDocument();
    setSql('SELECT * FROM "form_expenses" WHERE field_amount >= 7');
    fireEvent.click(
      await screen.findAllByRole("button", { name: "Edit condition" }).then(
        (buttons) => buttons[0],
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Value" })).toHaveValue("7")
    );
    expect(screen.queryByRole("button", { name: "Apply changes" })).toBeNull();
  });

  it("moves focus into and back out of a compact condition editor", async () => {
    const [sql, setSql] = createSignal(
      'SELECT * FROM "form_expenses" WHERE field_amount >= 4',
    );
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [context]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const row = await screen.findByRole("group", { name: "Amount" });
    const editButton = within(row).getByRole("button", {
      name: "Edit condition",
    });
    fireEvent.click(editButton);
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(row).getByRole("combobox", { name: "Column" }),
      )
    );
    fireEvent.click(within(row).getByRole("button", { name: "Done" }));
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(row).getByRole("button", { name: "Edit condition" }),
      )
    );
  });

  it("keeps multiple SELECT scopes on Advanced instead of offering cross-scope filters", async () => {
    const [sql] = createSignal(
      'SELECT field_amount FROM "form_expenses" WHERE field_amount > 1 UNION SELECT field_amount FROM "form_expenses" WHERE field_amount > 2',
    );
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [context]}
        onSqlChange={() => {}}
        onAdvanced={() => {}}
      />
    ));
    await screen.findByRole("button", { name: "Advanced structure…" });
    expect(screen.queryByRole("button", { name: "Add filter" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Add group" })).toBeNull();
  });

  it("retains filter undo history when the workspace unmounts and reopens", async () => {
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    const [visible, setVisible] = createSignal(true);
    const [history, setHistory] = createSignal<
      SqlConditionWorkspaceHistoryEntry[]
    >([]);
    const [redo, setRedo] = createSignal<SqlConditionWorkspaceHistoryEntry[]>(
      [],
    );
    render(() => (
      <Show when={visible()}>
        <SqlFilterSortEditor
          action="filter"
          sql={sql}
          scope={() => "saved-query:query-a"}
          contexts={() => [context]}
          onSqlChange={setSql}
          onAdvanced={() => {}}
          historyState={{ history, setHistory, redo, setRedo }}
        />
      </Show>
    ));
    fireEvent.change(
      await screen.findByRole("combobox", { name: "Column for new filter" }),
      {
        target: { value: "field_amount" },
      },
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Value for new filter" }),
      {
        target: { value: "12" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    await waitFor(() => expect(sql()).toContain("WHERE"));
    expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled();
    setVisible(false);
    setVisible(true);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(sql()).toBe('SELECT * FROM "form_expenses"'));
  });

  it("creates an Any group only after both inline conditions are valid", async () => {
    const statusContext: SqlStructureAdapterContext = {
      ...context,
      parser: {
        ...context.parser,
        columns: [{ name: "status", type: "string" }],
      },
      columnCapabilities: {
        status: {
          ...context.columnCapabilities.field_amount!,
          supportedOperators: ["eq", "not_eq", "is_null", "is_not_null"],
        },
      },
      columnLabels: { status: "Status" },
    };
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [statusContext]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    fireEvent.change(
      await screen.findByRole("combobox", { name: "Column for new filter" }),
      { target: { value: "status" } },
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Value for new filter" }),
      { target: { value: "open" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add group" }));
    const composer = await screen.findByRole("group", {
      name: "New filter group",
    });
    expect(within(composer).getByRole("button", { name: "Create group" }))
      .toBeDisabled();
    fireEvent.change(
      within(composer).getByRole("combobox", { name: "Filter group operator" }),
      { target: { value: "or" } },
    );
    fireEvent.change(
      within(composer).getByRole("combobox", {
        name: "Column for second condition",
      }),
      { target: { value: "status" } },
    );
    fireEvent.input(
      within(composer).getByRole("textbox", {
        name: "Value for second condition",
      }),
      { target: { value: "blocked" } },
    );
    expect(within(composer).getByRole("button", { name: "Create group" }))
      .toBeEnabled();
    fireEvent.click(
      within(composer).getByRole("button", { name: "Create group" }),
    );
    await waitFor(() =>
      expect(sql()).toMatch(/status\s*=\s*'open'.*OR.*status\s*=\s*'blocked'/i)
    );
    const connector = await screen.findByRole("combobox", {
      name: "Filter group operator",
    });
    expect(connector).toHaveValue("or");
  });

  it("opens a nested group composer from an existing root group", async () => {
    const statusContext: SqlStructureAdapterContext = {
      ...context,
      parser: {
        ...context.parser,
        columns: [{ name: "status", type: "string" }],
      },
      columnCapabilities: {
        status: {
          ...context.columnCapabilities.field_amount!,
          supportedOperators: ["eq", "is_null", "is_not_null"],
        },
      },
      columnLabels: { status: "Status" },
    };
    const [sql, setSql] = createSignal(
      `SELECT * FROM "form_expenses" WHERE status = 'open' AND status = 'blocked'`,
    );
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [statusContext]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const rootGroup = await screen.findByRole("group", {
      name: "Filter group",
    });
    fireEvent.click(screen.getByRole("button", { name: "Add condition" }));
    expect(
      screen.getByRole("combobox", { name: "Column for new filter" }),
    ).toBeInTheDocument();
    fireEvent.click(
      within(rootGroup).getByRole("button", { name: "Add group" }),
    );
    expect(
      await screen.findByRole("group", { name: "New filter group" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("combobox", { name: "Column for new filter" }),
    ).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Cancel", exact: true }),
    );
    expect(screen.queryByRole("group", { name: "New filter group" }))
      .toBeNull();
  });

  it("keeps the nested connector after undo and redo reuse condition IDs", async () => {
    const statusContext: SqlStructureAdapterContext = {
      ...context,
      parser: {
        ...context.parser,
        columns: [{ name: "status", type: "string" }],
      },
      columnCapabilities: {
        status: {
          ...context.columnCapabilities.field_amount!,
          supportedOperators: ["eq", "is_null", "is_not_null"],
        },
      },
      columnLabels: { status: "Status" },
    };
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [statusContext]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    fireEvent.change(
      await screen.findByRole("combobox", { name: "Column for new filter" }),
      { target: { value: "status" } },
    );
    fireEvent.input(
      screen.getByRole("textbox", { name: "Value for new filter" }),
      { target: { value: "open" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add group" }));
    const rootComposer = await screen.findByRole("group", {
      name: "New filter group",
    });
    fireEvent.change(
      within(rootComposer).getByRole("combobox", {
        name: "Column for second condition",
      }),
      { target: { value: "status" } },
    );
    fireEvent.input(
      within(rootComposer).getByRole("textbox", {
        name: "Value for second condition",
      }),
      { target: { value: "blocked" } },
    );
    fireEvent.click(
      within(rootComposer).getByRole("button", { name: "Create group" }),
    );
    await waitFor(() => expect(sql()).toContain("status"));
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(sql()).toBe('SELECT * FROM "form_expenses"'));
    fireEvent.click(screen.getByRole("button", { name: "Redo" }));
    await waitFor(() => expect(sql()).toContain("blocked"));

    const rootGroup = await screen.findByRole("group", {
      name: "Filter group",
    });
    fireEvent.click(
      within(rootGroup).getByRole("button", { name: "Add group" }),
    );
    const nestedComposer = await screen.findByRole("group", {
      name: "New filter group",
    });
    fireEvent.change(
      within(nestedComposer).getByRole("combobox", {
        name: "Filter group operator",
      }),
      { target: { value: "or" } },
    );
    expect(
      within(nestedComposer).getByRole("combobox", {
        name: "Filter group operator",
      }),
    ).toHaveValue("or");
    fireEvent.change(
      within(nestedComposer).getByRole("combobox", {
        name: "Column for second condition",
      }),
      { target: { value: "status" } },
    );
    fireEvent.input(
      within(nestedComposer).getByRole("textbox", {
        name: "Value for second condition",
      }),
      { target: { value: "pending" } },
    );
    fireEvent.click(
      within(nestedComposer).getByRole("button", { name: "Create group" }),
    );
    await waitFor(() => expect(sql()).toMatch(/status.*OR.*status/i));
    await waitFor(() => {
      const connectors = screen.getAllByRole("combobox", {
        name: "Filter group operator",
      });
      expect(connectors[0]).toHaveValue("and");
      expect(connectors.at(-1)).toHaveValue("or");
    });
  });

  it("adds ordered fields through the shared task editor", async () => {
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    render(() => (
      <SqlFilterSortEditor
        action="sort"
        sql={sql}
        contexts={() => [context]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const sortColumn = await screen.findByRole("combobox", { name: "Column" });
    await waitFor(() =>
      expect(sortColumn.querySelectorAll("option").length).toBe(2)
    );
    fireEvent.change(sortColumn, {
      target: { value: "field_amount" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Direction" }), {
      target: { value: "desc" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Nulls" }), {
      target: { value: "last" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add sort" }));
    await waitFor(() =>
      expect(sql()).toBe(
        'SELECT * FROM "form_expenses" ORDER BY field_amount DESC NULLS LAST',
      )
    );
  });

  it("edits and removes an existing simple sort row", async () => {
    const [sql, setSql] = createSignal(
      'SELECT * FROM "form_expenses" ORDER BY field_amount ASC',
    );
    render(() => (
      <SqlFilterSortEditor
        action="sort"
        sql={sql}
        contexts={() => [context]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const row = await screen.findByRole("group", { name: "Sort 1" });
    const direction = within(row).getByRole("combobox", { name: "Direction" });
    fireEvent.change(direction, { target: { value: "desc" } });
    fireEvent.click(
      within(row).getByRole("button", { name: "Apply sort changes" }),
    );
    await waitFor(() => expect(sql()).toContain("ORDER BY field_amount DESC"));
  });

  it("removes a simple existing sort row through the typed Rust edit", async () => {
    const [sql, setSql] = createSignal(
      'SELECT * FROM "form_expenses" ORDER BY field_amount',
    );
    render(() => (
      <SqlFilterSortEditor
        action="sort"
        sql={sql}
        contexts={() => [context]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const row = await screen.findByRole("group", { name: "Sort 1" });
    fireEvent.click(within(row).getByRole("button", { name: "Remove sort" }));
    await waitFor(() => expect(sql()).toBe('SELECT * FROM "form_expenses" '));
  });

  it("uses the referenced Form alias and quotes typed field identifiers", async () => {
    const aliased: SqlStructureAdapterContext = {
      ...context,
      parser: {
        ...context.parser,
        relation: "Form Expenses",
        columns: [{ name: "display value", type: "string" }],
      },
      columnCapabilities: {
        "display value": {
          ...context.columnCapabilities.field_amount!,
          supportedOperators: ["eq", "is_null", "is_not_null"],
        },
      },
      columnLabels: { "display value": "Display value" },
    };
    const unrelated = {
      ...context,
      parser: { ...context.parser, relation: "other_form" },
    };
    const [sql, setSql] = createSignal(
      `SELECT * FROM "Form Expenses" AS e WHERE status = 'open' OR status = 'pending'`,
    );
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [unrelated, aliased]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    fireEvent.click(
      await screen.findAllByRole("button", { name: "Add condition" }).then(
        (buttons) => buttons.at(-1)!,
      ),
    );
    const columnSelect = await screen.findByRole("combobox", {
      name: "Column for new filter",
    });
    await waitFor(() =>
      expect(columnSelect.querySelectorAll("option").length).toBe(2)
    );
    expect(screen.queryByRole("option", { name: "field_amount" })).toBeNull();
    fireEvent.change(columnSelect, { target: { value: "display value" } });
    fireEvent.input(
      screen.getByRole("textbox", { name: "Value for new filter" }),
      {
        target: { value: "closed" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    await waitFor(() =>
      expect(sql()).toContain(
        `WHERE (status = 'open' OR status = 'pending') AND (e."display value" = 'closed')`,
      )
    );
  });

  it("filters choices by per-field action capability and supported operators", async () => {
    const restricted: SqlStructureAdapterContext = {
      ...context,
      parser: {
        ...context.parser,
        columns: [
          { name: "field_amount", type: "integer" },
          { name: "secret", type: "string" },
        ],
      },
      columnCapabilities: {
        field_amount: {
          ...context.columnCapabilities.field_amount!,
          sortable: false,
          supportedOperators: ["eq"],
        },
        secret: {
          ...context.columnCapabilities.field_amount!,
          filterable: false,
          sortable: true,
        },
      },
    };
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    const { unmount } = render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [restricted]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const filterColumn = await screen.findByRole("combobox", {
      name: "Column for new filter",
    });
    await waitFor(() =>
      expect(filterColumn.querySelectorAll("option").length).toBe(2)
    );
    expect(screen.queryByRole("option", { name: "secret" })).toBeNull();
    fireEvent.change(filterColumn, { target: { value: "field_amount" } });
    const filterOperators = screen.getByRole("combobox", {
      name: "Operator for new filter",
    });
    await waitFor(() =>
      expect(filterOperators.querySelectorAll("option").length).toBe(1)
    );
    unmount();
    render(() => (
      <SqlFilterSortEditor
        action="sort"
        sql={sql}
        contexts={() => [restricted]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const sortColumn = await screen.findByRole("combobox", { name: "Column" });
    await waitFor(() =>
      expect(sortColumn.querySelectorAll("option").length).toBe(2)
    );
    expect(screen.queryByRole("option", { name: "field_amount" })).toBeNull();
  });

  it("rejects a selected filter field if its capability is revoked before submit", async () => {
    const [allowed, setAllowed] = createSignal(true);
    const [sql, setSql] = createSignal('SELECT * FROM "form_expenses"');
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [{
          ...context,
          columnCapabilities: {
            field_amount: {
              ...context.columnCapabilities.field_amount!,
              filterable: allowed(),
            },
          },
        }]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const column = await screen.findByRole("combobox", {
      name: "Column for new filter",
    });
    fireEvent.change(column, { target: { value: "field_amount" } });
    setAllowed(false);
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
    expect(sql()).toBe('SELECT * FROM "form_expenses"');
  });

  it("does not authorize duplicate relation basenames from another schema", async () => {
    const otherSchema = {
      ...context,
      parser: { ...context.parser, relation: "tenant_b.Form" },
    };
    const [sql, setSql] = createSignal("SELECT * FROM tenant_a.Form");
    render(() => (
      <SqlFilterSortEditor
        action="filter"
        sql={sql}
        contexts={() => [otherSchema]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    await waitFor(() =>
      expect(screen.queryByRole("option", { name: "field_amount" })).toBeNull()
    );
    expect(screen.queryByRole("button", { name: "Add filter" })).toBeNull();
    expect(screen.getByRole("button", { name: "Advanced structure…" }))
      .toBeInTheDocument();
    expect(sql()).toBe("SELECT * FROM tenant_a.Form");
  });

  it.each(["submit", "sort edit", "undo"] as const)(
    "does not apply a deferred %s result after the editor is disposed",
    async (path) => {
      const onSqlChange = vi.fn();
      const [sql, setSql] = createSignal(
        path === "sort edit"
          ? 'SELECT * FROM "form_expenses" ORDER BY field_amount'
          : 'SELECT * FROM "form_expenses"',
      );
      const view = render(() => (
        <SqlFilterSortEditor
          action={path === "sort edit" ? "sort" : "filter"}
          sql={sql}
          contexts={() => [context]}
          onSqlChange={(value) => {
            onSqlChange(value);
            setSql(value);
          }}
          onAdvanced={() => {}}
        />
      ));

      if (path === "undo") {
        const column = await screen.findByRole("combobox", {
          name: path === "sort edit" ? "Column" : "Column for new filter",
        });
        await waitFor(() =>
          expect(column.querySelectorAll("option").length).toBeGreaterThan(1)
        );
        fireEvent.change(column, { target: { value: "field_amount" } });
        await waitFor(() => expect(column).toHaveValue("field_amount"));
        fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
        await waitFor(() => expect(sql()).toContain("WHERE"));
        onSqlChange.mockClear();
      }

      const started = deferred<void>();
      if (path === "undo") {
        const pending = deferred<
          Awaited<ReturnType<typeof client.applySqlDocumentEditTransaction>>
        >();
        vi.spyOn(client, "applySqlDocumentEditTransaction").mockImplementation(
          () => {
            started.resolve();
            return pending.promise;
          },
        );
        fireEvent.click(screen.getByRole("button", { name: "Undo" }));
        await started.promise;
        view.unmount();
        pending.resolve(undefined as never);
      } else {
        const pending = deferred<
          Awaited<
            ReturnType<typeof client.applySqlDocumentSemanticEditTransaction>
          >
        >();
        vi.spyOn(client, "applySqlDocumentSemanticEditTransaction")
          .mockImplementation(() => {
            started.resolve();
            return pending.promise;
          });
        if (path === "sort edit") {
          const row = await screen.findByRole("group", { name: "Sort 1" });
          fireEvent.click(
            within(row).getByRole("button", { name: "Remove sort" }),
          );
        } else {
          const column = await screen.findByRole("combobox", {
            name: "Column for new filter",
          });
          await waitFor(() =>
            expect(column.querySelectorAll("option").length).toBeGreaterThan(1)
          );
          fireEvent.change(column, { target: { value: "field_amount" } });
          await waitFor(() => expect(column).toHaveValue("field_amount"));
          fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
        }
        await started.promise;
        view.unmount();
        pending.resolve(undefined as never);
      }
      await Promise.resolve();
      expect(onSqlChange).not.toHaveBeenCalled();
      vi.restoreAllMocks();
    },
  );

  it("rejects sort callbacks from an old source or switched scope", async () => {
    const [sql, setSql] = createSignal(
      'SELECT * FROM "form_expenses" ORDER BY field_amount',
    );
    const [scope, setScope] = createSignal("saved-query-a");
    render(() => (
      <SqlFilterSortEditor
        action="sort"
        sql={sql}
        scope={scope}
        contexts={() => [context]}
        onSqlChange={setSql}
        onAdvanced={() => {}}
      />
    ));
    const oldRow = await screen.findByRole("group", { name: "Sort 1" });
    const oldDirection = within(oldRow).getByRole("combobox", {
      name: "Direction",
    });
    setSql('SELECT * FROM "form_expenses" ORDER BY field_amount NULLS LAST');
    fireEvent.change(oldDirection, { target: { value: "desc" } });
    const afterSourceChange = sql();
    setScope("saved-query-b");
    fireEvent.change(oldDirection, { target: { value: "asc" } });
    expect(sql()).toBe(afterSourceChange);
  });
});
