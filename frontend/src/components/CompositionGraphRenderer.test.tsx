import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSignal } from "solid-js";
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

  it("shows the current literal Text while a previous template parse is stale", async () => {
    const [currentDraft, setCurrentDraft] = createSignal(draft);
    render(() => (
      <CompositionGraphRenderer
        draft={currentDraft()}
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
    setCurrentDraft({
      ...draft,
      components: draft.components.map((component) =>
        component.id === "text_1" && component.kind === "text"
          ? { ...component, content: "Current text" }
          : component
      ),
    });

    expect(screen.getByRole("heading", { name: "Current text" }))
      .toBeVisible();
  });

  it("uses the common no-value display for a null scalar", async () => {
    const nullDraft: CompositionGraphDraft = {
      ...draft,
      variables: draft.variables.map((node) =>
        node.kind === "query" && node.id === "total"
          ? {
            ...node,
            returns: {
              kind: "scalar",
              value_type: { scalar_type: "integer", nullable: true },
            },
          }
          : node
      ),
    };
    const nullEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      query_results: {
        ...evaluation.query_results,
        total: {
          value: { kind: "scalar", value: { kind: "null" } },
          has_more: false,
        },
      },
    };
    render(() => (
      <CompositionGraphRenderer
        draft={nullDraft}
        evaluation={nullEvaluation}
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

    expect(await screen.findByText(/Total: No data/)).toBeVisible();
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

  it("renders displayable scalar types together as one plain Text value", async () => {
    const values = [
      { id: "name", scalar_type: "string", value: { kind: "string", value: "<img src=x onerror=alert(1)> & Co." }, display: "<img src=x onerror=alert(1)> & Co." },
      { id: "active", scalar_type: "boolean", value: { kind: "boolean", value: true }, display: "true" },
      { id: "count", scalar_type: "integer", value: { kind: "integer", value: "9223372036854775807" }, display: "9223372036854775807" },
      { id: "precise", scalar_type: "decimal", value: { kind: "decimal", value: "12345678901234567890.0000000000000001" }, display: "12345678901234567890.0000000000000001" },
      { id: "today", scalar_type: "date", value: { kind: "date", value: "2026-11-17" }, display: "2026-11-17" },
      { id: "created", scalar_type: "timestamp", value: { kind: "timestamp", value: "2026-11-17T09:30:45.123456Z" }, display: "2026-11-17T09:30:45.123456Z" },
    ] as const;
    const content = values.map(({ id }) => `{{${id}}}`).join(" | ");
    const expected = values.map(({ display }) => display).join(" | ");
    const segments = values.flatMap((value, index) => [
      { kind: "variable" as const, variable: value.id },
      ...(index < values.length - 1
        ? [{ kind: "literal" as const, text: " | " }]
        : []),
    ]);
    const scalarDraft: CompositionGraphDraft = {
      ...draft,
      variables: values.map((value) => ({
        id: value.id,
        kind: "constant" as const,
        value_type: { scalar_type: value.scalar_type, nullable: false as const },
        value: value.value,
      })),
      components: [{
        id: "text_scalars",
        kind: "text",
        content,
        style: "body",
      }],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [{ kind: "component", component: "text_scalars" }],
        }],
      },
    };
    const scalarEvaluation: CompositionEvaluationResponse = {
      ...evaluation,
      component_bindings: [{
        component_id: "text_scalars",
        kind: "text",
        text_segments: segments,
      }],
    };
    const { container } = render(() => (
      <CompositionGraphRenderer
        draft={scalarDraft}
        evaluation={scalarEvaluation}
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

    const textValue = await screen.findByText(expected, { exact: true });
    expect(textValue.textContent).toBe(expected);
    expect(container.querySelector("img")).toBeNull();
    expect([...textValue.childNodes].filter((node) => node.nodeType === Node.TEXT_NODE)).toHaveLength(1);
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

  it("shows a request failure beside the affected Text result", async () => {
    render(() => (
      <CompositionGraphRenderer
        draft={draft}
        evaluation={evaluation}
        queryErrors={{ total: "SQL query execution failed" }}
        rawInputs={{}}
        inputStates={{}}
        blockedQueryIds={new Set(["total"])}
        previousPageQueryIds={new Set()}
        pageBusy={false}
        onInput={vi.fn()}
        onNext={vi.fn()}
        onPrevious={vi.fn()}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "SQL query execution failed",
    );
  });
});
