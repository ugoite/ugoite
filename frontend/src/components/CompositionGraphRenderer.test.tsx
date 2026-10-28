import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
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
      default: { kind: "integer", value: "1" },
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
  variable_values: { minimum: { kind: "integer", value: "1" } },
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
          [{ kind: "string", value: "Alpha" }, { kind: "integer", value: "4" }],
          [{ kind: "string", value: "Beta" }, { kind: "integer", value: "7" }],
        ],
      },
      has_more: true,
      next: "cursor-2",
    },
    total: {
      value: { kind: "scalar", value: { kind: "integer", value: "2" } },
      has_more: false,
    },
  },
};

describe("CompositionGraphRenderer", () => {
  beforeEach(() => setLocale("en"));
  afterEach(() => cleanup());

  it("renders typed query values and requests continuation by variable and cursor", () => {
    const onNext = vi.fn();
    const onPrevious = vi.fn();
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={evaluation}
        rawInputs={{ minimum: "2" }}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set(["rows"])}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={onNext}
        onPrevious={onPrevious}
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
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(onPrevious).toHaveBeenCalledWith("rows");
  });

  it("renders i64 values exactly in Table cells and Metrics", () => {
    const maximum = "9223372036854775807";
    const largeEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      query_results: {
        ...evaluation.query_results,
        rows: {
          ...evaluation.query_results.rows,
          value: {
            kind: "table",
            columns: evaluation.query_results.rows.value.kind === "table"
              ? evaluation.query_results.rows.value.columns
              : [],
            rows: [[
              { kind: "string", value: "Large" },
              { kind: "integer", value: maximum },
            ]],
          },
        },
        total: {
          value: { kind: "scalar", value: { kind: "integer", value: maximum } },
          has_more: false,
        },
      },
    };
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={largeEvaluation}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(screen.getByRole("cell", { name: maximum })).toBeInTheDocument();
    expect(screen.getByLabelText("total")).toHaveTextContent(maximum);
  });

  it("announces an empty table after successful evaluation without keeping previous rows", () => {
    const [result, setResult] = createSignal(evaluation);
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={result()}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));
    expect(screen.getByRole("table")).toHaveTextContent("Alpha");
    setResult({
      ...evaluation,
      query_results: {
        ...evaluation.query_results,
        rows: {
          value: {
            ...evaluation.query_results.rows.value,
            kind: "table",
            rows: [],
          },
          has_more: false,
        },
      },
    });
    expect(screen.getByRole("table")).not.toHaveTextContent("Alpha");
    expect(screen.getByRole("status")).toHaveTextContent(
      "The query returned no rows.",
    );
    expect(screen.queryByRole("button", { name: "Next" })).not
      .toBeInTheDocument();
  });

  it("shows the selected Saved SQL output alias for an opaque generated variable", () => {
    const variableId =
      "query_018ff2a7-32d4-7b80-a57f-03010544ea9b_opaque_value";
    const savedSqlDraft: CompositionGraphDraft = {
      ...draft,
      variables: draft.variables.map((node) =>
        node.id === "total"
          ? {
            ...node,
            id: variableId,
            query: {
              kind: "saved_sql",
              entry_id: "018ff2a7-32d4-7b80-a57f-03010544ea9b",
              revision_id: "2fbf7d50-5a29-4805-a936-41bf2702a908",
              column: "total_amount",
              arguments: [],
            },
          }
          : node
      ),
      components: draft.components.map((component) =>
        component.kind === "metric"
          ? { ...component, variable: variableId }
          : component
      ),
    };
    const savedSqlEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      computed_query_ids: ["rows", variableId],
      query_results: {
        ...evaluation.query_results,
        [variableId]: {
          value: { kind: "scalar", value: { kind: "integer", value: "20" } },
          has_more: false,
        },
      },
    };

    render(() => (
      <CompositionGraphRenderer
        draft={savedSqlDraft}
        evaluation={savedSqlEvaluation}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(screen.getByRole("heading", { name: "total_amount", exact: true }))
      .toBeVisible();
    expect(screen.getByRole("region", { name: "total_amount", exact: true }))
      .toHaveTextContent("20");
    expect(screen.queryByRole("region", { name: variableId }))
      .not.toBeInTheDocument();
  });

  it("uses the declared variable name for a Saved SQL metric with a named ID", () => {
    const variableId = "all_rows";
    const namedVariableDraft: CompositionGraphDraft = {
      ...draft,
      variables: draft.variables.map((node) =>
        node.id === "total"
          ? {
            ...node,
            id: variableId,
            query: {
              kind: "saved_sql",
              entry_id: "018ff2a7-32d4-7b80-a57f-03010544ea9b",
              revision_id: "2fbf7d50-5a29-4805-a936-41bf2702a908",
              column: "row_count",
              arguments: [],
            },
          }
          : node
      ),
      components: draft.components.map((component) =>
        component.kind === "metric"
          ? { ...component, variable: variableId }
          : component
      ),
    };
    const namedVariableEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      computed_query_ids: ["rows", variableId],
      query_results: {
        ...evaluation.query_results,
        [variableId]: {
          value: { kind: "scalar", value: { kind: "integer", value: "3" } },
          has_more: false,
        },
      },
    };

    render(() => (
      <CompositionGraphRenderer
        draft={namedVariableDraft}
        evaluation={namedVariableEvaluation}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(screen.getByRole("heading", { name: variableId, exact: true }))
      .toBeVisible();
    expect(screen.getByRole("region", { name: variableId, exact: true }))
      .toHaveTextContent("3");
    expect(screen.queryByRole("region", { name: "row_count" }))
      .not.toBeInTheDocument();
  });

  it("does not expose a generated ID for a direct Saved SQL scalar metric", () => {
    const variableId = "query_018ff2a7-32d4-7b80-a57f-03010544ea9b_value";
    const directScalarDraft: CompositionGraphDraft = {
      ...draft,
      variables: draft.variables.map((node) =>
        node.id === "total"
          ? {
            ...node,
            id: variableId,
            query: {
              kind: "saved_sql",
              entry_id: "018ff2a7-32d4-7b80-a57f-03010544ea9b",
              revision_id: "2fbf7d50-5a29-4805-a936-41bf2702a908",
              arguments: [],
            },
          }
          : node
      ),
      components: draft.components.map((component) =>
        component.kind === "metric"
          ? { ...component, variable: variableId }
          : component
      ),
    };
    const directScalarEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      computed_query_ids: ["rows", variableId],
      query_results: {
        ...evaluation.query_results,
        [variableId]: {
          value: { kind: "scalar", value: { kind: "integer", value: "20" } },
          has_more: false,
        },
      },
    };

    render(() => (
      <CompositionGraphRenderer
        draft={directScalarDraft}
        evaluation={directScalarEvaluation}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(screen.getByRole("heading", { name: "Metric result", exact: true }))
      .toBeVisible();
    expect(screen.getByRole("region", { name: "Metric result", exact: true }))
      .toHaveTextContent("20");
    expect(screen.queryByText(variableId)).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: variableId })).not
      .toBeInTheDocument();
  });

  it("renders the current raw string input in text bindings before evaluation", () => {
    const textDraft: CompositionGraphDraft = {
      name: "Report",
      variables: [{
        id: "report_title",
        kind: "input",
        value_type: { scalar_type: "string", nullable: false },
        required: true,
        default: { kind: "string", value: "Previous title" },
      }],
      components: [
        { id: "title_input", kind: "input", variable: "report_title" },
        {
          id: "title_text",
          kind: "text",
          content: { kind: "variable", variable: "report_title" },
          style: "body",
        },
      ],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [
            { kind: "component", component: "title_input" },
            { kind: "component", component: "title_text" },
          ],
        }],
      },
    };
    const [rawInputs, setRawInputs] = createSignal<Record<string, string>>({});
    const [inputStates, setInputStates] = createSignal<
      Record<string, "pending" | "invalid" | "missing">
    >({});
    render(() => (
      <CompositionGraphRenderer
        draft={textDraft}
        evaluation={{
          ...evaluation,
          computed_query_ids: [],
          variable_values: {
            report_title: { kind: "string", value: "Previous title" },
          },
          query_results: {},
        }}
        rawInputs={rawInputs()}
        inputStates={inputStates()}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={(variableId, raw) => {
          setRawInputs((current) => ({ ...current, [variableId]: raw }));
          setInputStates((current) => ({
            ...current,
            [variableId]: "pending",
          }));
        }}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    const input = screen.getByRole("textbox", { name: "report_title" });
    expect(screen.getByText("Previous title")).toBeVisible();

    fireEvent.input(input, { target: { value: "Transient report title" } });

    expect(input).toHaveValue("Transient report title");
    expect(screen.getByText("Transient report title")).toBeVisible();
    expect(screen.queryByText("Previous title")).not.toBeInTheDocument();
  });

  it("renders a one-row NULL scalar distinctly from zero rows", () => {
    render(() => (
      <CompositionGraphRenderer
        draft={{
          ...draft,
          variables: draft.variables.map((node) =>
            node.id === "total" && node.kind === "query"
              ? {
                ...node,
                returns: {
                  kind: "scalar",
                  value_type: { scalar_type: "integer", nullable: true },
                },
              }
              : node
          ),
        }}
        evaluation={{
          ...evaluation,
          query_results: {
            ...evaluation.query_results,
            total: {
              value: { kind: "scalar", value: { kind: "null" } },
              has_more: false,
            },
          },
        }}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(screen.getByLabelText("total")).toHaveTextContent("No data");
    expect(screen.getByLabelText("total")).not.toHaveTextContent("0");
  });

  it("surfaces a query diagnostic in its local component", () => {
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={{
          ...evaluation,
          query_diagnostics: [{
            code: "sql_error",
            variable_id: "total",
            result_diagnostic: {
              code: "result_error",
              result_diagnostic: { code: "scalar_multiple_rows" },
            },
          }],
        }}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "The metric returned more than one row.",
    );
    expect(screen.getByRole("table", { name: "rows results" })).toBeVisible();
  });

  it("shows a local aborted state for successful siblings of a failed selected evaluation", () => {
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={{
          ...evaluation,
          ok: false,
          query_results: {},
          query_diagnostics: [
            { code: "evaluation_aborted", variable_id: "total" },
            { code: "result_validation_failed", variable_id: "rows" },
          ],
        }}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));
    expect(
      screen.getAllByRole("alert").some((alert) =>
        alert.textContent ===
          "Another query failed. This evaluation did not produce a result."
      ),
    ).toBe(true);
    expect(screen.queryByText("Loading")).not.toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("hides stale metrics while an invalid input blocks their query", () => {
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={evaluation}
        rawInputs={{ minimum: "not an integer" }}
        inputStates={{ minimum: "invalid" }}
        blockedQueryIds={new Set(["total"])}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(screen.queryByLabelText("total")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for input…",
    );
  });
  it("hides stale scalar displays for an invalid input while leaving independent results usable", () => {
    const graph = structuredClone(draft);
    graph.components.push({
      id: "input-value",
      kind: "metric",
      variable: "minimum",
    });
    graph.layout.rows[0].items.push({
      kind: "component",
      component: "input-value",
    });
    render(() => (
      <CompositionGraphRenderer
        draft={graph}
        evaluation={evaluation}
        rawInputs={{ minimum: "bad" }}
        inputStates={{ minimum: "invalid" }}
        blockedQueryIds={new Set()}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));
    expect(screen.getByRole("textbox", { name: "minimum" })).toHaveValue("bad");
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for input");
    expect(screen.getByRole("table")).toHaveTextContent("Alpha");
    expect(screen.getByRole("region", { name: "total" })).toHaveTextContent(
      "2",
    );
  });
});
