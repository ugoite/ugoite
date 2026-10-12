import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DashboardFlowRenderer } from "./DashboardFlowRenderer";
import { compositionApi } from "~/lib/composition-api";
import { setLocale } from "~/lib/i18n";
import type {
  CompositionParameterDefinition,
  CompositionResolvePlan,
} from "~/lib/composition-api";

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

const definitions: CompositionParameterDefinition[] = [
  {
    id: "month",
    label: "Month",
    type: "date",
    required: true,
    default: "2026-10-01",
    format: "year-month",
  },
  {
    id: "region",
    label: "Region",
    type: "string",
    required: false,
  },
];

const plan: CompositionResolvePlan = {
  composition_revision: { entry_id: "tool-1", revision_id: "revision-1" },
  sources: [{
    kind: "saved_sql",
    source_id: "summary",
    source_schema_fingerprint: "fingerprint",
    request: {
      sql: "SELECT total, count",
      limit: 1,
      saved_sql: { id: "sql-1", revision_id: "sql-revision-1" },
    },
  }],
  component_bindings: [
    {
      component_id: "summary_title",
      kind: "text",
      label: "Summary",
    },
    {
      component_id: "total",
      kind: "metric",
      label: "Total",
      source_id: "summary",
      result_property_key: "total",
      expected_result_type: "integer",
    },
    {
      component_id: "count",
      kind: "metric",
      label: "Count",
      source_id: "summary",
      result_property_key: "count",
      expected_result_type: "integer",
    },
    {
      component_id: "rows",
      kind: "table",
      label: "Details",
      source_id: "summary",
    },
  ],
};

const texts = {
  summary_title: { text: "Monthly summary", style: "heading" as const },
};

const readySources = {
  summary: {
    status: "ready" as const,
    cursorStack: [undefined],
    page: {
      kind: "saved_sql" as const,
      page: {
        columns: ["total", "count"],
        rows: [{ total: 42, count: 7 }],
        has_more: false,
      },
    },
  },
};

const renderFlow = (
  overrides: Partial<Parameters<typeof DashboardFlowRenderer>[0]> = {},
) =>
  render(() => (
    <DashboardFlowRenderer
      layout={{
        rows: [
          {
            id: "controls",
            items: [{ kind: "parameter", parameter: "month" }],
          },
          {
            id: "heading",
            items: [{ kind: "component", component: "summary_title" }],
          },
          {
            id: "summary",
            items: [
              { kind: "component", component: "total" },
              { kind: "component", component: "count" },
            ],
          },
          {
            id: "detail",
            items: [{ kind: "component", component: "rows" }],
          },
        ],
      }}
      plan={plan}
      texts={texts}
      parameterDefinitions={definitions}
      parameterValues={{ month: "2026-10-01" }}
      onParameterChange={() => {}}
      sources={readySources}
      onNext={() => {}}
      onPrevious={() => {}}
      onRetry={() => {}}
      {...overrides}
    />
  ));

describe("DashboardFlowRenderer", () => {
  afterEach(() => vi.restoreAllMocks());

  it("renders rows in order with parameter controls interleaved", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockImplementation(
      (_spaceId, request) =>
        Promise.resolve({
          ok: true,
          result: {
            component_id: request.component_id,
            operation: "sql_scalar",
            source_kind: "saved_sql",
            result_type: "integer",
            value: request.component_id === "total" ? "42" : "7",
          },
        }),
    );
    const { container } = renderFlow({
      metricQueryContext: {
        spaceId: "space-1",
        target: {
          kind: "saved_revision",
          composition_id: "tool-1",
          revision_id: "revision-1",
        },
        parameters: { month: "2026-10-01" },
      },
    });

    const rows = Array.from(
      container.querySelectorAll(".compositionFlowRow"),
    ).map((row) => row.textContent);
    expect(rows).toHaveLength(4);
    expect(rows[0]).toContain("Month");
    expect(rows[1]).toContain("Monthly summary");
    expect(rows[2]).toContain("Total");
    expect(rows[2]).toContain("Count");
    expect(rows[3]).toContain("Details");

    // The parameter control shows the declared human label, never the id.
    const input = screen.getByLabelText("Month");
    expect(input).toHaveValue("2026-10-01");
    expect(screen.queryByLabelText("month")).toBeNull();
    // Unplaced optional parameters render no in-flow control.
    expect(screen.queryByLabelText("Region")).toBeNull();

    // Metrics query the bounded server operation; the table delegates to
    // the source-native presenter.
    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    expect(await screen.findByText("7", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(2);
    expect(query).toHaveBeenCalledWith(
      "space-1",
      expect.objectContaining({
        target: {
          kind: "saved_revision",
          composition_id: "tool-1",
          revision_id: "revision-1",
        },
        parameters: { month: "2026-10-01" },
      }),
      expect.any(AbortSignal),
    );
    expect(screen.getByRole("table")).toBeInTheDocument();
  });

  it("uses localized ordinal labels for unlabeled runtime parameters", async () => {
    setLocale("en");
    const unlabeledDefinitions = definitions.map((
      { label: _label, ...entry },
    ) => entry);
    renderFlow({ parameterDefinitions: unlabeledDefinitions });

    expect(await screen.findByLabelText("Parameter 1")).toHaveValue(
      "2026-10-01",
    );
    expect(screen.queryByLabelText("month")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("region")).not.toBeInTheDocument();

    setLocale("ja");
    expect(screen.getByLabelText("パラメーター1")).toHaveValue("2026-10-01");
  });

  it("renders each text style without fetching sources", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric");
    renderFlow({
      layout: {
        rows: [{
          id: "notes",
          items: [
            { kind: "component", component: "title_text" },
            { kind: "component", component: "heading_text" },
            { kind: "component", component: "body_text" },
            { kind: "component", component: "caption_text" },
          ],
        }],
      },
      plan: {
        sources: [],
        component_bindings: [
          { component_id: "title_text", kind: "text" },
          { component_id: "heading_text", kind: "text" },
          { component_id: "body_text", kind: "text" },
          { component_id: "caption_text", kind: "text" },
        ],
      },
      texts: {
        title_text: { text: "Monthly expenses", style: "title" },
        heading_text: { text: "Summary", style: "heading" },
        body_text: { text: "Settled monthly.", style: "body" },
        caption_text: { text: "Unaudited.", style: "caption" },
      },
      sources: {},
    });

    expect(screen.getByRole("heading", { name: "Monthly expenses" }))
      .toHaveClass("flowText--title");
    expect(screen.getByRole("heading", { name: "Summary" })).toHaveClass(
      "flowText--heading",
    );
    expect(screen.getByText("Settled monthly.")).toHaveClass("flowText--body");
    expect(screen.getByText("Unaudited.")).toHaveClass("flowText--caption");
    await waitFor(() => expect(query).not.toHaveBeenCalled());
  });

  it("binds parameter changes to transient work state", () => {
    const onParameterChange = vi.fn();
    renderFlow({
      parameterDefinitions: [{
        id: "include_archived",
        label: "Include archived",
        type: "boolean",
        required: true,
      }],
      layout: {
        rows: [{
          id: "controls",
          items: [{ kind: "parameter", parameter: "include_archived" }],
        }],
      },
      parameterValues: {},
      onParameterChange,
    });

    const select = screen.getByLabelText("Include archived");
    fireEvent.change(select, { target: { value: "false" } });
    expect(onParameterChange).toHaveBeenCalledWith(
      "include_archived",
      false,
    );
  });

  it("delegates flow metrics and tables to source-native presenters", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "sql_scalar",
        source_kind: "saved_sql",
        result_type: "integer",
        value: "42",
      },
    });
    renderFlow({
      layout: {
        rows: [{
          id: "summary",
          items: [
            { kind: "component", component: "total" },
            { kind: "component", component: "rows" },
          ],
        }],
      },
      metricQueryContext: {
        spaceId: "space-1",
        target: {
          kind: "saved_revision",
          composition_id: "tool-1",
          revision_id: "revision-1",
        },
        parameters: { month: "2026-10-01" },
      },
    });

    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(1);
    expect(query).toHaveBeenCalledWith(
      "space-1",
      expect.objectContaining({ component_id: "total" }),
      expect.any(AbortSignal),
    );
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "42" })).toBeInTheDocument();
  });

  it("uses exact-revision scalar queries in flow layout and preserves the table page", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "sql_scalar",
        source_kind: "saved_sql",
        result_type: "integer",
        value: "42",
      },
    });
    renderFlow({
      layout: {
        rows: [{
          id: "summary",
          items: [
            { kind: "component", component: "total" },
            { kind: "component", component: "rows" },
          ],
        }],
      },
      metricQueryContext: {
        spaceId: "space-1",
        target: {
          kind: "saved_revision",
          composition_id: "tool-1",
          revision_id: "revision-1",
        },
        parameters: { month: "2026-10-01" },
      },
    });

    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(1);
    expect(query).toHaveBeenCalledWith(
      "space-1",
      {
        target: {
          kind: "saved_revision",
          composition_id: "tool-1",
          revision_id: "revision-1",
        },
        component_id: "total",
        parameters: { month: "2026-10-01" },
      },
      expect.any(AbortSignal),
    );
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "42" })).toBeInTheDocument();
  });

  it("keeps a server metric stable when its table advances to another page", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "sql_scalar",
        source_kind: "saved_sql",
        result_type: "integer",
        value: "42",
      },
    });
    const firstPage = {
      summary: {
        status: "ready" as const,
        cursorStack: [undefined],
        page: {
          kind: "saved_sql" as const,
          page: {
            columns: ["total", "count"],
            rows: [{ total: 42, count: 7 }],
            has_more: true,
            next: "page-2",
          },
        },
      },
    };
    const secondPage = {
      summary: {
        status: "ready" as const,
        cursorStack: [undefined, "page-2"],
        cursor: "page-2",
        page: {
          kind: "saved_sql" as const,
          page: {
            columns: ["total", "count"],
            rows: [{ total: 84, count: 14 }],
            has_more: false,
          },
        },
      },
    };
    const [sources, setSources] = createSignal(firstPage);
    render(() => (
      <DashboardFlowRenderer
        layout={{
          rows: [{
            id: "summary",
            items: [
              { kind: "component", component: "total" },
              { kind: "component", component: "rows" },
            ],
          }],
        }}
        plan={plan}
        texts={texts}
        parameterDefinitions={definitions}
        parameterValues={{ month: "2026-10-01" }}
        onParameterChange={() => {}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: { month: "2026-10-01" },
        }}
        sources={sources()}
        onNext={() => setSources(secondPage)}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "42" })).toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await waitFor(() => {
      expect(screen.getByRole("cell", { name: "84" })).toBeInTheDocument();
    });
    expect(screen.getByText("42", { selector: "output" })).toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(1);
  });

  it("passes saved field types to an all-null Entry table in flow layout", () => {
    const entrySource: CompositionResolvePlan["sources"][number] = {
      kind: "entry_query",
      source_id: "entries",
      source_schema_fingerprint: "entry-fingerprint",
      request: {
        query: {
          scope: { kind: "form", form_id: "form-1" },
          filters: [],
          sort: [],
        },
        projection: {
          kind: "fields",
          fields: [{ kind: "property", field_id: 9 }],
        },
        limit: 1,
      },
    };
    renderFlow({
      layout: {
        rows: [{
          id: "entries",
          items: [{ kind: "component", component: "entries" }],
        }],
      },
      plan: {
        sources: [entrySource],
        component_bindings: [{
          component_id: "entries",
          kind: "table",
          label: "Entries",
          source_id: "entries",
        }],
      },
      parameterDefinitions: [],
      parameterValues: {},
      sources: {
        entries: {
          status: "ready",
          cursorStack: [undefined],
          page: {
            kind: "entry_query",
            page: {
              rows: [{
                id: "entry-1",
                form_id: "form-1",
                revision_id: "revision-1",
                created_at_micros: 1,
                updated_at_micros: 1,
                properties: { amount: null },
              }],
              has_more: false,
            },
          },
        },
      },
      fieldNames: (_formId, fieldId) => fieldId === 9 ? "Amount" : undefined,
      fieldKeys: (_formId, fieldId) => fieldId === 9 ? "amount" : undefined,
      fieldTypes: (_formId, fieldId, sourceId) =>
        sourceId === "entries" && fieldId === 9 ? "double" : undefined,
    });

    expect(screen.getByRole("columnheader", { name: "Amount" })).toHaveClass(
      "result-table-header--numeric",
    );
    expect(screen.getByRole("cell", { name: "—" })).toHaveClass(
      "result-table-cell--numeric",
    );
  });

  it("shows an explicit empty state for an empty layout", () => {
    renderFlow({ layout: { rows: [] } });

    expect(screen.getByText("No results")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("skips unknown layout references without failing the row", () => {
    renderFlow({
      layout: {
        rows: [{
          id: "broken",
          items: [
            { kind: "component", component: "missing_component" },
            { kind: "parameter", parameter: "missing_parameter" },
          ],
        }],
      },
    });

    expect(document.querySelector(".compositionFlowRow")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("keeps one loading indicator when flow blocks share a source", () => {
    renderFlow({
      sources: {
        summary: { status: "loading", cursorStack: [undefined] },
      },
    });

    expect(screen.getAllByRole("status")).toHaveLength(1);
  });

  it("keeps flow rows inline with tables scrolling in their own viewport", () => {
    const css = stylesheet();
    // Desktop rows keep items inline with wrapping; tables take the full
    // row while metrics share it and parameter controls keep content width.
    expect(css).toMatch(
      /\.compositionFlowRow\s*\{[^}]*display:\s*flex[^}]*flex-wrap:\s*wrap/,
    );
    expect(css).toMatch(
      /\.compositionFlowItem--table\s*\{[^}]*flex:\s*1\s*1\s*100%/,
    );
    expect(css).toMatch(
      /\.result-table-viewport\s*\{[^}]*overflow-x:\s*auto/,
    );
    // Narrow viewports stack metric and parameter blocks instead of
    // squeezing them or scrolling the document.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.compositionFlowItem--metric\s*\{[\s\S]*?flex-basis:\s*100%/,
    );
  });
});
