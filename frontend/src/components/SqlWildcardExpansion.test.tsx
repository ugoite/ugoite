import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as client from "~/lib/ugoite-client";
import type { PlanSqlColumns } from "~/lib/sql-document-columns";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import { SqlColumnsEditor } from "./SqlColumnsEditor";
import { SqlDocumentTreeEditor } from "./SqlDocumentTreeEditor";

const { applyMock } = vi.hoisted(() => ({ applyMock: vi.fn() }));
vi.mock("~/lib/ugoite-client", async (original) => {
  const actual = await original<typeof client>();
  applyMock.mockImplementation(actual.applySqlDocumentSemanticEditTransaction);
  return { ...actual, applySqlDocumentSemanticEditTransaction: applyMock };
});
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
      supportedOperators: [],
      aggregateFunctions: [],
      groupable: true,
      distinctable: true,
    },
  },
  columnLabels: { field_amount: "Amount" },
};
function setup(
  source: string,
  mode: "guided" | "advanced",
  plan: PlanSqlColumns,
) {
  const [sql, setSql] = createSignal(source);
  const [scope, setScope] = createSignal("query-a");
  render(() =>
    mode === "guided"
      ? (
        <SqlColumnsEditor
          sql={sql}
          scope={scope}
          contexts={() => [context]}
          onSqlChange={setSql}
          onPlanColumns={plan}
          onAdvanced={() => {}}
        />
      )
      : (
        <SqlDocumentTreeEditor
          sql={sql}
          scope={scope}
          relationOptions={() => [{
            relation: "form_expenses",
            label: "Expenses",
            columns: ["field_amount"],
          }]}
          onSqlChange={setSql}
          onPlanColumns={plan}
        />
      )
  );
  return { sql, setScope };
}
async function expansion(mode: "guided" | "advanced") {
  if (mode === "guided") {
    return await screen.findByRole("button", {
      name: "Choose fields",
      exact: true,
    });
  }
  fireEvent.click(
    await screen.findByRole("treeitem", { name: /Selected expression:.*\*/ }),
  );
  return await screen.findByRole("button", {
    name: "Expand wildcard",
    exact: true,
  });
}
for (const mode of ["guided", "advanced"] as const) {
  describe(`${mode} wildcard expansion through the actual Rust document`, () => {
    beforeEach(async () => {
      vi.clearAllMocks();
      const actual = await vi.importActual<typeof client>(
        "~/lib/ugoite-client",
      );
      applyMock.mockImplementation(
        actual.applySqlDocumentSemanticEditTransaction,
      );
    });
    it.each([
      'SELECT * FROM "form_expenses" e JOIN (SELECT 1 AS x) q ON 1=1',
      'SELECT * FROM "form_expenses" e JOIN "missing_form" q ON 1=1',
      'SELECT * FROM "form_expenses" e JOIN public.other q ON 1=1',
      "WITH form_expenses AS (SELECT 1 AS x) SELECT * FROM form_expenses",
      'SELECT *, 1 AS extra FROM "form_expenses"',
    ])("never replaces only the known subset of %s", async (source) => {
      const plan = vi.fn();
      const { sql } = setup(source, mode, plan);
      if (mode === "guided") {
        await screen.findByRole("list", { name: "Result fields" });
        expect(screen.queryByRole("button", { name: "Choose fields" })).not
          .toBeInTheDocument();
      } else {
        expect(await expansion(mode)).toBeDisabled();
      }
      expect(plan).not.toHaveBeenCalled();
      expect(applyMock).not.toHaveBeenCalled();
      expect(sql()).toBe(source);
    });
    it("uses the qualified wildcard's complete planned output order including residual fields", async () => {
      const source =
        'SELECT e.* FROM "form_expenses" e JOIN "other_form" i ON 1=1';
      const plan = vi.fn(async () => ({
        columns: ["residual_field", "field_amount", "_ugoite_id"],
        isCurrent: () => true,
      }));
      const { sql } = setup(source, mode, plan);
      fireEvent.click(await expansion(mode));
      if (mode === "advanced") {
        fireEvent.click(
          screen.getByRole("button", { name: "Apply", exact: true }),
        );
      }
      await waitFor(() =>
        expect(sql()).toBe(
          'SELECT e."residual_field", e."field_amount", e."_ugoite_id" FROM "form_expenses" e JOIN "other_form" i ON 1=1',
        )
      );
      expect(plan).toHaveBeenCalledOnce();
      expect(applyMock).toHaveBeenCalledOnce();
    });
    it.each(
      [[], ["amount", "amount"], [""], ["bad\0name"]].map((columns) => ({
        columns,
      })),
    )(
      "retains source when the planned names are ambiguous: %j",
      async ({ columns }) => {
        const source = 'SELECT * FROM "form_expenses"';
        const { sql } = setup(
          source,
          mode,
          async () => ({ columns, isCurrent: () => true }),
        );
        fireEvent.click(await expansion(mode));
        if (mode === "advanced") {
          fireEvent.click(
            screen.getByRole("button", { name: "Apply", exact: true }),
          );
        }
        expect(await screen.findByRole("alert")).toBeInTheDocument();
        expect(sql()).toBe(source);
        expect(applyMock).not.toHaveBeenCalled();
      },
    );
    it("discards a pending schema after the host changes", async () => {
      const source = 'SELECT * FROM "form_expenses"';
      let finish!: (value: Awaited<ReturnType<PlanSqlColumns>>) => void;
      const plan = vi.fn(() =>
        new Promise<Awaited<ReturnType<PlanSqlColumns>>>((resolve) => {
          finish = resolve;
        })
      );
      const { sql, setScope } = setup(source, mode, plan);
      fireEvent.click(await expansion(mode));
      if (mode === "advanced") {
        fireEvent.click(
          screen.getByRole("button", { name: "Apply", exact: true }),
        );
      }
      await waitFor(() => expect(plan).toHaveBeenCalledOnce());
      setScope("query-b");
      finish({ columns: ["field_amount"], isCurrent: () => true });
      await waitFor(() => expect(sql()).toBe(source));
      expect(applyMock).not.toHaveBeenCalled();
    });
    it("discards a plan after its parameter snapshot becomes stale", async () => {
      const source = 'SELECT * FROM "form_expenses"';
      const { sql } = setup(
        source,
        mode,
        async () => ({ columns: ["field_amount"], isCurrent: () => false }),
      );
      fireEvent.click(await expansion(mode));
      if (mode === "advanced") {
        fireEvent.click(
          screen.getByRole("button", { name: "Apply", exact: true }),
        );
      }
      expect(await screen.findByRole("alert")).toBeInTheDocument();
      expect(sql()).toBe(source);
      expect(applyMock).not.toHaveBeenCalled();
    });
  });
}
