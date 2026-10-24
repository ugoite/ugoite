import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
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
  columnCapabilities: {},
  columnLabels: { field_amount: "Amount" },
};

describe("SQL Filter and Sort task editors", () => {
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
    fireEvent.change(await screen.findByRole("combobox", { name: "Column" }), {
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
        'SELECT * FROM "form_expenses" WHERE form_expenses.field_amount >= 9007199254740993',
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
    fireEvent.change(await screen.findByRole("combobox", { name: "Column" }), {
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
        'SELECT * FROM "form_expenses" ORDER BY form_expenses.field_amount DESC NULLS LAST',
      )
    );
  });
});
