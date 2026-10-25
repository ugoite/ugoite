import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
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
      name: "Column",
    });
    await waitFor(() =>
      expect(filterColumn.querySelectorAll("option").length).toBe(2)
    );
    fireEvent.change(filterColumn, { target: { value: "field_amount" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Filter value" }), {
      target: { value: "argument" },
    });
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
      name: "Column",
    });
    await waitFor(() =>
      expect(filterColumn.querySelectorAll("option").length).toBe(2)
    );
    fireEvent.change(filterColumn, {
      target: { value: "field_amount" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Operator" }), {
      target: { value: "gte" },
    });
    const value = screen.getByRole("textbox", { name: "Value" });
    fireEvent.change(screen.getByRole("combobox", { name: "Value type" }), {
      target: { value: "number" },
    });
    fireEvent.input(value, { target: { value: "9007199254740993" } });
    expect(screen.getByRole("textbox", { name: "Value" })).toBe(value);
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
    const columnSelect = await screen.findByRole("combobox", {
      name: "Column",
    });
    await waitFor(() =>
      expect(columnSelect.querySelectorAll("option").length).toBe(2)
    );
    expect(screen.queryByRole("option", { name: "field_amount" })).toBeNull();
    fireEvent.change(columnSelect, { target: { value: "display value" } });
    fireEvent.input(screen.getByRole("textbox", { name: "Value" }), {
      target: { value: "closed" },
    });
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
      name: "Column",
    });
    await waitFor(() =>
      expect(filterColumn.querySelectorAll("option").length).toBe(2)
    );
    expect(screen.queryByRole("option", { name: "secret" })).toBeNull();
    fireEvent.change(filterColumn, { target: { value: "field_amount" } });
    const filterOperators = screen.getByRole("combobox", { name: "Operator" });
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
    const column = await screen.findByRole("combobox", { name: "Column" });
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
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
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
        const column = await screen.findByRole("combobox", { name: "Column" });
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
            name: "Column",
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
