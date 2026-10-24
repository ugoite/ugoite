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
import * as client from "~/lib/ugoite-client";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import { SqlColumnsEditor } from "./SqlColumnsEditor";

const { applyMock } = vi.hoisted(() => ({ applyMock: vi.fn() }));
vi.mock("~/lib/ugoite-client", async (original) => {
  const actual = await original<typeof client>();
  applyMock.mockImplementation(actual.applySqlDocumentSemanticEditTransaction);
  return { ...actual, applySqlDocumentSemanticEditTransaction: applyMock };
});
const capabilities = {
  projectable: true,
  filterable: true,
  sortable: true,
  supportedOperators: [],
  aggregateFunctions: [],
  groupable: true,
  distinctable: true,
};
const contexts: SqlStructureAdapterContext[] = [{
  identity: "expenses",
  relationSelectable: true,
  label: "Expenses",
  parser: {
    relation: "form_expenses",
    columns: [{ name: "field_amount", type: "integer" }, {
      name: "field_day",
      type: "date",
    }],
    variables: [],
    stable_key: [],
  },
  columnCapabilities: { field_amount: capabilities, field_day: capabilities },
  columnLabels: { field_amount: "Amount", field_day: "Day" },
}, {
  identity: "invoices",
  relationSelectable: true,
  label: "Invoices",
  parser: {
    relation: "form_invoices",
    columns: [{ name: "field_total", type: "integer" }],
    variables: [],
    stable_key: [],
  },
  columnCapabilities: { field_total: capabilities },
  columnLabels: { field_total: "Total" },
}];
function setup(source: string) {
  const [sql, setSql] = createSignal(source);
  const [scope, setScope] = createSignal("space-a:query-1");
  const advanced = vi.fn();
  render(() => (
    <SqlColumnsEditor
      sql={sql}
      scope={scope}
      contexts={() => contexts}
      onSqlChange={setSql}
      onPlanColumns={async () => ({
        columns: ["field_amount", "field_day"],
        isCurrent: () => true,
      })}
      onAdvanced={advanced}
    />
  ));
  return { sql, setSql, setScope, advanced };
}

describe("Columns task view through the actual Rust SQL document", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    const actual = await vi.importActual<typeof client>("~/lib/ugoite-client");
    applyMock.mockImplementation(
      actual.applySqlDocumentSemanticEditTransaction,
    );
  });
  it("shows human field names, edits output order, removes fields, and undoes without changing other clauses", async () => {
    const source =
      '-- keep\nSELECT "field_amount", "field_day" FROM "form_expenses" WHERE TRUE ORDER BY "field_amount" DESC';
    const { sql } = setup(source);
    const list = await screen.findByRole("list", { name: "Result fields" });
    await waitFor(() =>
      expect(within(list).getAllByRole("listitem")).toHaveLength(2)
    );
    expect(within(list).getAllByRole("listitem")[0]).toHaveTextContent(
      "Amount",
    );
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Move Amount down" }));
    await waitFor(() =>
      expect(sql()).toBe(
        '-- keep\nSELECT "field_day", "field_amount" FROM "form_expenses" WHERE TRUE ORDER BY "field_amount" DESC',
      )
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Remove Day" })).not
        .toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove Day" }));
    await waitFor(() =>
      expect(sql()).toMatch(
        /^-- keep\nSELECT\s+"field_amount" FROM "form_expenses" WHERE TRUE ORDER BY "field_amount" DESC$/,
      )
    );
    expect(screen.getByRole("button", { name: "Remove Amount" }))
      .toHaveAttribute("aria-disabled", "true");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo", exact: true }))
        .not.toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo", exact: true }));
    await waitFor(() =>
      expect(within(list).getAllByRole("listitem")).toHaveLength(2)
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo", exact: true }))
        .not.toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo", exact: true }));
    await waitFor(() => expect(sql()).toBe(source));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Redo", exact: true })).not
        .toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Redo", exact: true }));
    await waitFor(() =>
      expect(sql()).toContain('SELECT "field_day", "field_amount"')
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo", exact: true })).not
        .toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo", exact: true }));
    await waitFor(() => expect(sql()).toBe(source));
  });
  it("expands only the qualified wildcard's Form, preserving joins and their aliases", async () => {
    const source =
      'SELECT e.* FROM "form_expenses" AS e JOIN "form_invoices" AS i ON 1=1';
    const { sql } = setup(source);
    fireEvent.click(
      await screen.findByRole("button", { name: "Choose fields" }),
    );
    await waitFor(() =>
      expect(sql()).toContain('SELECT e."field_amount", e."field_day" FROM')
    );
    expect(sql()).not.toContain('i."field_total"');
    expect(sql()).toContain(
      'FROM "form_expenses" AS e JOIN "form_invoices" AS i ON 1=1',
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Add a field" }))
        .toBeEnabled()
    );
    fireEvent.change(screen.getByRole("combobox", { name: "Add a field" }), {
      target: { value: "1:field_total" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add", exact: true }));
    await waitFor(() => expect(sql()).toContain('i."field_total"'));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo", exact: true }))
        .not.toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo", exact: true }));
    await waitFor(() => expect(sql()).not.toContain('i."field_total"'));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Undo", exact: true }))
        .not.toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Undo", exact: true }));
    await waitFor(() => expect(sql()).toBe(source));
  });
  it("retains wildcard modifiers and exposes advanced editing without guessing their expansion", async () => {
    const source = 'SELECT * EXCEPT (field_day) FROM "form_expenses"';
    const { sql, advanced } = setup(source);
    await screen.findByText("* EXCEPT (field_day)");
    expect(screen.queryByRole("button", { name: "Choose fields" })).not
      .toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Advanced structure…" }),
    );
    expect(advanced).toHaveBeenCalledOnce();
    expect(sql()).toBe(source);
    expect(applyMock).not.toHaveBeenCalled();
  });
  it("keeps nested SELECT field choices scoped to their own Form", async () => {
    const { sql } = setup(
      'SELECT (SELECT "field_total" FROM "form_invoices") AS total, "field_amount" FROM "form_expenses"',
    );
    const results = await screen.findByRole("combobox", {
      name: "Results to edit",
    });
    expect(
      within(screen.getByRole("combobox", { name: "Add a field" }))
        .getAllByRole("option").map((option) => option.textContent),
    ).toEqual(["Amount", "Day"]);
    fireEvent.change(results, { target: { value: "1" } });
    expect(
      within(screen.getByRole("combobox", { name: "Add a field" }))
        .getAllByRole("option").map((option) => option.textContent),
    ).toEqual(["Total"]);
    fireEvent.click(screen.getByRole("button", { name: "Add", exact: true }));
    await waitFor(() =>
      expect(sql()).toContain(
        'SELECT "field_total", "form_invoices"."field_total" FROM',
      )
    );
    expect(sql()).toContain('"field_amount" FROM "form_expenses"');
  });
  it("preserves positional sorting and rejects removing its referenced field with a local cause", async () => {
    const source =
      'SELECT "field_amount", "field_day" FROM "form_expenses" ORDER BY 1 DESC';
    const { sql } = setup(source);
    fireEvent.click(
      await screen.findByRole("button", { name: "Move Amount down" }),
    );
    await waitFor(() => expect(sql()).toContain("ORDER BY 2 DESC"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Remove Amount" })).not
        .toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove Amount" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "referenced by position",
    );
    expect(sql()).toContain('SELECT "field_day", "field_amount"');
    expect(sql()).toContain("ORDER BY 2 DESC");
    fireEvent.click(screen.getByRole("button", { name: "Undo", exact: true }));
    await waitFor(() => expect(sql()).toBe(source));
  });
  it("keeps focus inside the editor after a focused field is removed", async () => {
    const { sql } = setup(
      'SELECT "field_amount", "field_day" FROM "form_expenses"',
    );
    const remove = await screen.findByRole("button", { name: "Remove Day" });
    remove.focus();
    fireEvent.click(remove);
    await waitFor(() => expect(sql()).not.toContain('"field_day"'));
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Add a field" }))
        .toBeEnabled()
    );
    expect(globalThis.document.activeElement?.closest(".sql-columns-editor"))
      .not.toBeNull();
    expect(remove.isConnected).toBe(false);
  });
  it("keeps the same field's focused control through a pending reorder", async () => {
    const source = 'SELECT "field_amount", "field_day" FROM "form_expenses"';
    const { sql } = setup(source);
    const actual = await vi.importActual<typeof client>("~/lib/ugoite-client");
    let finish!: () => void;
    applyMock.mockImplementationOnce(async (...args) => {
      const result = await actual.applySqlDocumentSemanticEditTransaction(
        ...args,
      );
      await new Promise<void>((resolve) => {
        finish = resolve;
      });
      return result;
    });
    const button = await screen.findByRole("button", {
      name: "Move Amount down",
    });
    button.focus();
    fireEvent.click(button);
    await waitFor(() =>
      expect(button).toHaveAttribute("aria-disabled", "true")
    );
    expect(button).toHaveFocus();
    fireEvent.click(button);
    expect(applyMock).toHaveBeenCalledOnce();
    await waitFor(() => expect(finish).toBeDefined());
    finish();
    await waitFor(() =>
      expect(sql()).toContain('SELECT "field_day", "field_amount"')
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Move Amount up" })).not
        .toHaveAttribute("aria-disabled", "true")
    );
    expect(screen.getByRole("button", { name: "Move Amount down" })).toBe(
      button,
    );
    expect(button).toHaveFocus();
  });
  it("rejects a pending edit after host identity changes even with identical SQL", async () => {
    const source = 'SELECT "field_amount", "field_day" FROM "form_expenses"';
    const { sql, setScope } = setup(source);
    let finish!: (value: unknown) => void;
    applyMock.mockImplementationOnce(() =>
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Move Amount down" }),
    );
    await waitFor(() => expect(applyMock).toHaveBeenCalledOnce());
    setScope("space-b:query-1");
    finish({
      document: await client.parseSqlDocument("SELECT 9"),
      undo: { expected_source: "SELECT 9", edits: [] },
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Move Amount down" }))
        .not.toHaveAttribute("aria-disabled", "true")
    );
    expect(sql()).toBe(source);
    expect(screen.getByRole("button", { name: "Undo", exact: true }))
      .toHaveAttribute("aria-disabled", "true");
  });
});
