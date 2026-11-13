import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as client from "~/lib/ugoite-client";
import type { Form } from "~/lib/types";
import { SqlFormSourceControl } from "./SqlFormSourceControl";

const { seedMock } = vi.hoisted(() => ({ seedMock: vi.fn() }));
vi.mock("~/lib/ugoite-client", async (importOriginal) => {
  const actual = await importOriginal<typeof client>();
  seedMock.mockImplementation(actual.compositionFormQuerySql);
  return { ...actual, compositionFormQuerySql: seedMock };
});

const expensesForm: Form = {
  id: "form-expenses-id",
  name: "Expenses",
  sql_relation: "form_expenses",
  version: 1,
  template: "",
  fields: {
    amount: {
      type: "integer",
      required: false,
      label: "Amount paid",
      sql_column: "amount",
      query_capability: {
        field: { kind: "property", field_id: 1 },
        name: "amount",
        field_type: "integer",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals"],
      },
    },
  },
};

function setup(sql: string, forms: readonly Form[]) {
  const [currentSql, setCurrentSql] = createSignal(sql);
  const selected = vi.fn();
  render(() => (
    <SqlFormSourceControl
      sql={currentSql}
      forms={() => forms}
      onFormBindingSelected={(selection) => {
        setCurrentSql(selection.sql);
        selected(selection);
      }}
    />
  ));
  return { currentSql, selected };
}

describe("SQL Form source control", () => {
  beforeEach(async () => {
    const actual = await vi.importActual<typeof client>("~/lib/ugoite-client");
    seedMock.mockReset().mockImplementation(actual.compositionFormQuerySql);
  });

  it("provides a Form starter without SQL construction actions", () => {
    setup("SELECT 1", [expensesForm]);
    expect(screen.getByRole("button", { name: "Form" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Arguments" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Columns" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Filter" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Sort" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Structure" })).toBeNull();
  });

  it("seeds explicit projectable columns and retains the selected Form identity", async () => {
    const sql =
      'SELECT "amount" AS "Amount paid" FROM "Expenses" ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"';
    seedMock.mockResolvedValueOnce({ sql });
    const { currentSql, selected } = setup("SELECT 1", [expensesForm]);

    fireEvent.click(screen.getByRole("button", { name: "Form" }));
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));

    await waitFor(() =>
      expect(seedMock).toHaveBeenCalledExactlyOnceWith(
        "Expenses",
        [{ column: "amount", label: "Amount paid" }],
      )
    );
    expect(currentSql()).toBe(sql);
    expect(selected).toHaveBeenCalledWith({
      sql,
      name: "Expenses",
      form_id: "form-expenses-id",
    });
  });

  it("keeps a Form seed disabled after SQL text has changed", () => {
    setup('SELECT "amount" FROM "Expenses"', [expensesForm]);
    expect(screen.getByRole("button", { name: "Form" })).toBeDisabled();
    expect(seedMock).not.toHaveBeenCalled();
  });

  it("shows the Rust validation when a Form has no projectable fields", async () => {
    const { selected } = setup("SELECT 1", [{
      id: "form-system-id",
      name: "System records",
      sql_relation: "form_system_records",
      version: 1,
      template: "",
      fields: {},
    }]);

    fireEvent.click(screen.getByRole("button", { name: "Form" }));
    fireEvent.click(await screen.findByRole("button", { name: "System records" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This Form has no displayable data fields.",
    );
    expect(seedMock).toHaveBeenCalledExactlyOnceWith("System records", []);
    expect(selected).not.toHaveBeenCalled();
  });
});
