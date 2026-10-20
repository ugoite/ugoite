import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CompositionGraphRenderer } from "./CompositionGraphRenderer";
import type { CompositionEvaluationResponse } from "~/lib/composition-api";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

const draft: CompositionGraphDraft = {
  ...createCompositionGraphDraft("Report"),
  variables: [
    {
      id: "minimum",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      required: true,
      default: { kind: "integer", value: 1 },
    },
    {
      id: "rows",
      kind: "query",
      returns: {
        kind: "table",
        columns: [
          {
            name: "name",
            value_type: { scalar_type: "string", nullable: false },
          },
          {
            name: "amount",
            value_type: { scalar_type: "integer", nullable: false },
          },
        ],
      },
      query: { kind: "inline_sql", sql: "SELECT name, amount", arguments: [] },
    },
    {
      id: "total",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: { kind: "inline_sql", sql: "SELECT COUNT(*)", arguments: [] },
    },
  ],
  components: [
    { id: "input_1", kind: "input", variable: "minimum" },
    { id: "table_1", kind: "table", variable: "rows" },
    { id: "metric_1", kind: "metric", variable: "total" },
  ],
  layout: {
    kind: "flow",
    rows: [{
      id: "main",
      items: [
        { kind: "component", component: "input_1" },
        { kind: "component", component: "metric_1" },
        { kind: "component", component: "table_1" },
      ],
    }],
  },
};

const evaluation: CompositionEvaluationResponse = {
  evaluation_id: "evaluation-1",
  ok: true,
  computed_query_ids: ["rows", "total"],
  variable_values: { minimum: { kind: "integer", value: 1 } },
  queries: [],
  component_bindings: [],
  dependencies: {},
  query_results: {
    rows: {
      value: {
        kind: "table",
        columns: [
          {
            name: "name",
            value_type: { scalar_type: "string", nullable: false },
          },
          {
            name: "amount",
            value_type: { scalar_type: "integer", nullable: false },
          },
        ],
        rows: [
          [{ kind: "string", value: "Alpha" }, { kind: "integer", value: 4 }],
          [{ kind: "string", value: "Beta" }, { kind: "integer", value: 7 }],
        ],
      },
      has_more: true,
      next: "cursor-2",
    },
    total: {
      value: { kind: "scalar", value: { kind: "integer", value: 2 } },
      has_more: false,
    },
  },
};

describe("CompositionGraphRenderer", () => {
  beforeEach(() => setLocale("en"));
  afterEach(() => cleanup());

  it("renders typed query values and requests continuation by variable and cursor", () => {
    const onNext = vi.fn();
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={evaluation}
        rawInputs={{ minimum: "2" }}
        inputStates={{}}
        blockedQueryIds={new Set()}
        onInput={vi.fn()}
        onNext={onNext}
      />
    ));

    expect(screen.getByRole("textbox", { name: "minimum" })).toHaveValue("2");
    expect(screen.getByLabelText("total")).toHaveTextContent("2");
    const table = screen.getByRole("table", { name: "rows results" });
    expect(table).toHaveTextContent("Alpha");
    expect(table).toHaveTextContent("Beta");
    expect(table.querySelectorAll("thead th")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(onNext).toHaveBeenCalledWith("rows", "cursor-2");
  });

  it("surfaces a query diagnostic in its local component", () => {
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={{
          ...evaluation,
          query_diagnostics: [{
            code: "sql_error",
            variable_id: "rows",
            result_diagnostic: {
              code: "result_error",
              result_diagnostic: { code: "planning_failed" },
            },
          }],
        }}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        onInput={vi.fn()}
        onNext={vi.fn()}
      />
    ));

    expect(screen.getByRole("alert")).toHaveTextContent("planning_failed");
    expect(screen.getByLabelText("total")).toHaveTextContent("2");
  });
});
