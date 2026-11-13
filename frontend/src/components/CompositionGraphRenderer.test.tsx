import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CompositionGraphRenderer } from "./CompositionGraphRenderer";
import type { CompositionEvaluationResponse } from "~/lib/composition-api";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

const text = "Total: {{total}} <img src=x onerror=alert(1)>";
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
          { name: "name", value_type: { scalar_type: "string", nullable: false } },
          { name: "amount", value_type: { scalar_type: "integer", nullable: false } },
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
    { id: "text_1", kind: "text", content: text, style: "heading" },
  ],
  layout: {
    kind: "flow",
    rows: [{
      id: "main",
      items: [
        { kind: "component", component: "input_1" },
        { kind: "component", component: "table_1" },
        { kind: "component", component: "text_1" },
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
  component_bindings: [{
    component_id: "text_1",
    kind: "text",
    text_segments: [
      { kind: "literal", text: "Total: " },
      { kind: "variable", variable: "total" },
      { kind: "literal", text: " <img src=x onerror=alert(1)>" },
    ],
  }],
  dependencies: {},
  query_results: {
    rows: {
      value: {
        kind: "table",
        columns: [
          { name: "name", value_type: { scalar_type: "string", nullable: false } },
          { name: "amount", value_type: { scalar_type: "integer", nullable: false } },
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

  it("renders Input and Table values and requests the next page", () => {
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
    const table = screen.getByRole("table", { name: "rows" });
    expect(table).toHaveTextContent("Alpha");
    expect(table).toHaveTextContent("Beta");
    expect(table.querySelectorAll("thead th")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(onNext).toHaveBeenCalledWith("rows", "cursor-2");
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(onPrevious).toHaveBeenCalledWith("rows");
  });

  it("renders scalar Text references as escaped plain text", async () => {
    const { container } = render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={evaluation}
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

    expect(await screen.findByRole("heading", { name: /Total: 2/ }))
      .toBeVisible();
    expect(screen.getByText(text.replace("{{total}}", "2"))).toBeVisible();
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders exact i64 and decimal scalar tokens without Number coercion", async () => {
    const maximum = "9223372036854775807";
    const decimal = "12345678901234567890.0000000000000001";
    const exactDraft: CompositionGraphDraft = {
      ...draft,
      variables: [
        {
          id: "maximum",
          kind: "constant",
          value_type: { scalar_type: "integer", nullable: false },
          value: { kind: "integer", value: maximum },
        },
        {
          id: "precise",
          kind: "constant",
          value_type: { scalar_type: "decimal", nullable: false },
          value: { kind: "decimal", value: decimal },
        },
      ],
      components: [{
        id: "text_exact",
        kind: "text",
        content: "{{maximum}} / {{precise}}",
        style: "body",
      }],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [{ kind: "component", component: "text_exact" }],
        }],
      },
    };
    const exactEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      component_bindings: [{
        component_id: "text_exact",
        kind: "text",
        text_segments: [
          { kind: "variable", variable: "maximum" },
          { kind: "literal", text: " / " },
          { kind: "variable", variable: "precise" },
        ],
      }],
    };
    render(() => (
      <CompositionGraphRenderer
        draft={exactDraft}
        evaluation={exactEvaluation}
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

    expect(await screen.findByText(`${maximum} / ${decimal}`)).toBeVisible();
  });

  it("keeps exact i64 result tokens in Table cells", () => {
    const maximum = "9223372036854775807";
    const exactEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      query_results: {
        ...evaluation.query_results,
        rows: {
          value: {
            kind: "table",
            columns: [
              { name: "name", value_type: { scalar_type: "string", nullable: false } },
              { name: "amount", value_type: { scalar_type: "integer", nullable: false } },
            ],
            rows: [[
              { kind: "string", value: "Large" },
              { kind: "integer", value: maximum },
            ]],
          },
          has_more: false,
        },
      },
    };
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={exactEvaluation}
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

    expect(screen.getByRole("cell", { name: maximum })).toBeVisible();
  });

  it("shows scalar execution diagnostics next to the referencing Text", async () => {
    const withDiagnostic: CompositionEvaluationResponse = {
      ...evaluation,
      query_diagnostics: [{
        code: "scalar_multiple_rows",
        variable_id: "total",
      }],
    };
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={withDiagnostic}
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

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The query returned more than one value.",
    );
  });
});
