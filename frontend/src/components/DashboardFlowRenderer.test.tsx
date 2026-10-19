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
  CompositionResolvePlan,
  CompositionVariableDefinition,
} from "~/lib/composition-api";

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

const definitions: CompositionVariableDefinition[] = [
  {
    id: "month",
    type: "date",
    required: true,
    default: "2026-10-01",
    format: "year-month",
  },
  {
    id: "region",
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
      component_id: "input-month",
      kind: "input",
      variable_id: "month",
    },
    {
      component_id: "summary_title",
      kind: "text",
    },
    {
      component_id: "total",
      kind: "metric",
      source_id: "summary",
      result_property_key: "total",
      expected_result_type: "integer",
    },
    {
      component_id: "count",
      kind: "metric",
      source_id: "summary",
      result_property_key: "count",
      expected_result_type: "integer",
    },
    {
      component_id: "rows",
      kind: "table",
      source_id: "summary",
    },
  ],
  dependencies: {
    variable_consumers: {
      month: { source_ids: ["summary"] },
      region: {},
    },
  },
};

const texts = {
  summary_title: { text: "Monthly summary", style: "heading" as const },
};

const readySources = {
  summary: {
    status: "ready" as const,
    cursorStack: [undefined],
    page: {
      kind: "sql" as const,
      page: {
        columns: ["total", "count"],
        rows: [{ total: 42, count: 7 }],
        result_schema: [
          { name: "total", type: "integer" as const },
          { name: "count", type: "integer" as const },
        ],
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
            items: [{ kind: "component", component: "input-month" }],
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
      variableDefinitions={definitions}
      variableValues={{ month: "2026-10-01" }}
      sources={readySources}
      onNext={() => {}}
      onPrevious={() => {}}
      onRetry={() => {}}
      {...overrides}
    />
  ));

describe("DashboardFlowRenderer", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    setLocale("en");
  });

  it("renders rows in order with Input controls interleaved", async () => {
    const onVariableInput = vi.fn();
    const query = vi.spyOn(compositionApi, "queryMetric").mockImplementation(
      (_spaceId, request) =>
        Promise.resolve({
          ok: true,
          result: {
            component_id: request.component_id,
            result_type: "integer",
            value: request.component_id === "total" ? "42" : "7",
          },
        }),
    );
    const { container } = renderFlow({
      onVariableInput,
      metricQueryContext: {
        spaceId: "space-1",
        target: {
          kind: "saved_revision",
          composition_id: "tool-1",
          revision_id: "revision-1",
        },
        variables: { month: "2026-10-01" },
      },
    });

    const rows = Array.from(
      container.querySelectorAll(".compositionFlowRow"),
    ).map((row) => row.textContent);
    expect(rows).toHaveLength(4);
    expect(rows[0]).toContain("month");
    expect(rows[1]).toContain("Monthly summary");
    expect(rows[2]).toContain("total");
    expect(rows[2]).toContain("count");
    expect(rows[3]).toContain("total");
    expect(rows[3]).toContain("count");

    // The Input uses the stable Composition variable ID.
    const input = screen.getByLabelText("month");
    expect(input).toHaveValue("2026-10-01");
    expect(input).not.toHaveAttribute("readonly");
    fireEvent.input(input, { target: { value: "2026-11-01" } });
    await waitFor(() => {
      expect(onVariableInput).toHaveBeenCalledWith("month", "2026-11-01");
    });
    // Unplaced optional variables render no in-flow control.
    expect(screen.queryByLabelText("region")).toBeNull();

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
        variables: { month: "2026-10-01" },
      }),
      expect.any(AbortSignal),
    );
    expect(screen.getByRole("table")).toBeInTheDocument();
  });

  it("uses stable variable IDs for runtime controls in every locale", async () => {
    setLocale("en");
    renderFlow({ variableDefinitions: definitions });

    expect(await screen.findByLabelText("month")).toHaveValue(
      "2026-10-01",
    );
    expect(screen.queryByLabelText("region")).not.toBeInTheDocument();

    setLocale("ja");
    expect(screen.getByLabelText("month")).toHaveValue("2026-10-01");
  });

  it("renders an Input when its resolved definition arrives after its binding", async () => {
    const [resolvedPlan, setResolvedPlan] = createSignal<CompositionResolvePlan>({
      sources: [],
      component_bindings: [],
    });
    const [resolvedDefinitions, setResolvedDefinitions] = createSignal<
      CompositionVariableDefinition[]
    >([]);

    render(() => (
      <DashboardFlowRenderer
        layout={{
          rows: [{
            id: "controls",
            items: [{ kind: "component", component: "input-month" }],
          }],
        }}
        inputVariables={{ "input-month": "month" }}
        plan={resolvedPlan()}
        texts={{}}
        variableDefinitions={resolvedDefinitions()}
        variableValues={{ month: "2026-10-01" }}
        sources={{}}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(screen.queryByLabelText("month")).not.toBeInTheDocument();
    setResolvedPlan({
      sources: [],
      component_bindings: [{
        component_id: "input-month",
        kind: "input",
        variable_id: "month",
      }],
    });
    setResolvedDefinitions([definitions[0]]);

    expect(await screen.findByLabelText("month")).toHaveValue("2026-10-01");
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

  it("binds Input changes to transient Work state", async () => {
    const onVariableInput = vi.fn();
    renderFlow({
      onVariableInput,
      variableDefinitions: [{
        id: "include_archived",
        type: "boolean",
        required: true,
      }],
      layout: {
        rows: [{
          id: "controls",
          items: [{ kind: "component", component: "input-include-archived" }],
        }],
      },
      variableValues: {},
      inputVariables: { "input-include-archived": "include_archived" },
      plan: {
        composition_revision: {
          entry_id: "tool-1",
          revision_id: "revision-1",
        },
        sources: [],
        component_bindings: [{
          component_id: "input-include-archived",
          kind: "input",
          variable_id: "include_archived",
        }],
      },
    });

    const input = screen.getByLabelText("include_archived");
    expect(input).toHaveValue("");
    expect(input).not.toHaveAttribute("readonly");
    expect(input).toHaveAttribute("aria-required", "true");
    fireEvent.input(input, { target: { value: "true" } });
    await waitFor(() => {
      expect(onVariableInput).toHaveBeenCalledWith("include_archived", "true");
    });
  });

  it("delegates flow metrics and tables to source-native presenters", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
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
        variables: { month: "2026-10-01" },
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
        variables: { month: "2026-10-01" },
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
        variables: { month: "2026-10-01" },
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
        result_type: "integer",
        value: "42",
      },
    });
    const firstPage = {
      summary: {
        status: "ready" as const,
        cursorStack: [undefined],
        page: {
          kind: "sql" as const,
          page: {
            columns: ["total", "count"],
            rows: [{ total: 42, count: 7 }],
            result_schema: [
              { name: "total", type: "integer" as const },
              { name: "count", type: "integer" as const },
            ],
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
          kind: "sql" as const,
          page: {
            columns: ["total", "count"],
            rows: [{ total: 84, count: 14 }],
            result_schema: [
              { name: "total", type: "integer" as const },
              { name: "count", type: "integer" as const },
            ],
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
        variableDefinitions={definitions}
        variableValues={{ month: "2026-10-01" }}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          variables: { month: "2026-10-01" },
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

  it("passes saved SQL result types to an all-null table in flow layout", () => {
    const sqlSource: CompositionResolvePlan["sources"][number] = {
      kind: "saved_sql",
      source_id: "entries",
      source_schema_fingerprint: "sql-fingerprint",
      expected_result: [{ name: "amount", type: "float" }],
      request: {
        sql: "SELECT amount FROM expenses",
        parameters: {},
        parameter_types: {},
        limit: 1,
        saved_sql: { id: "sql-2", revision_id: "sql-revision-2" },
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
        sources: [sqlSource],
        component_bindings: [{
          component_id: "entries",
          kind: "table",
          source_id: "entries",
        }],
      },
      variableDefinitions: [],
      variableValues: {},
      sources: {
        entries: {
          status: "ready",
          cursorStack: [undefined],
          page: {
            kind: "sql",
            page: {
              columns: ["amount"],
              rows: [[null]],
              result_schema: [{ name: "amount", type: "float" }],
              has_more: false,
            },
          },
        },
      },
    });

    expect(screen.getByRole("columnheader", { name: "amount" })).toHaveClass(
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
    // row while metrics share it and Input controls keep content width.
    expect(css).toMatch(
      /\.compositionFlowRow\s*\{[^}]*display:\s*flex[^}]*flex-wrap:\s*wrap/,
    );
    expect(css).toMatch(
      /\.compositionFlowItem--table\s*\{[^}]*flex:\s*1\s*1\s*100%/,
    );
    expect(css).toMatch(
      /\.result-table-viewport\s*\{[^}]*overflow-x:\s*auto/,
    );
    // Narrow viewports stack metric and Input blocks instead of
    // squeezing them or scrolling the document.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.compositionFlowItem--metric\s*\{[\s\S]*?flex-basis:\s*100%/,
    );
  });
});
