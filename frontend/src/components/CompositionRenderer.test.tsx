import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CompositionRenderer } from "./CompositionRenderer";
import { compositionApi } from "~/lib/composition-api";
import type { CompositionResolvePlan } from "~/lib/composition-api";

const plan: CompositionResolvePlan = {
  composition_revision: { entry_id: "tool-1", revision_id: "revision-1" },
  sources: [{
    kind: "saved_sql",
    source_id: "summary",
    source_schema_fingerprint: "fingerprint",
    request: {
      sql: "SELECT total",
      limit: 1,
      saved_sql: { id: "sql-1", revision_id: "sql-revision-1" },
    },
  }],
  component_bindings: [{
    component_id: "total",
    kind: "metric",
    label: "Total",
    source_id: "summary",
    result_property_key: "total",
    expected_result_type: "float",
  }, {
    component_id: "rows",
    kind: "table",
    label: "Details",
    source_id: "summary",
  }],
};

const sources = {
  summary: {
    status: "ready" as const,
    cursorStack: [undefined],
    page: {
      kind: "saved_sql" as const,
      page: {
        columns: ["total"],
        rows: [{ total: 42 }],
        has_more: false,
      },
    },
  },
};

describe("CompositionRenderer", () => {
  afterEach(() => vi.restoreAllMocks());

  it("renders section order and displays the shared WASM metric result", async () => {
    const evaluate = vi.spyOn(compositionApi, "evaluateMetricPage")
      .mockResolvedValue({ ok: true, value: 42 });
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={sources}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    await waitFor(() =>
      expect(evaluate).toHaveBeenCalledWith({
        expected_result_type: "float",
        is_complete: true,
        row_count: 1,
        selected_column_count: 1,
        selected_value: 42,
      })
    );
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) =>
        heading.textContent
      ),
    ).toEqual(["Total", "Details"]);
    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "42" })).toBeInTheDocument();
  });

  it("shows the stable evaluator diagnostic without aggregating result rows", async () => {
    vi.spyOn(compositionApi, "evaluateMetricPage").mockResolvedValue({
      ok: false,
      error: {
        kind: "composition_diagnostic",
        code: "metric_result_multiple_rows",
      },
    });
    const multipleRows = {
      summary: {
        ...sources.summary,
        page: {
          ...sources.summary.page,
          page: {
            columns: ["total"],
            rows: [{ total: 21 }, { total: 21 }],
            has_more: false,
          },
        },
      },
    };
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={multipleRows}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The metric returned more than one row.",
    );
    expect(screen.queryByText("42", { selector: "output" }))
      .not.toBeInTheDocument();
  });

  it("keeps the first-page metric when the table advances to a continuation page", async () => {
    const evaluate = vi.spyOn(compositionApi, "evaluateMetricPage")
      .mockResolvedValue({
        ok: false,
        error: {
          kind: "composition_diagnostic",
          code: "metric_result_page_incomplete",
        },
      });
    const [currentSources, setCurrentSources] = createSignal(sources);
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={currentSources()}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The metric result is incomplete.",
    );
    expect(evaluate).toHaveBeenCalledTimes(1);

    setCurrentSources({
      summary: {
        status: "ready",
        cursor: "opaque-next",
        cursorStack: [undefined, "opaque-next"],
        page: {
          kind: "saved_sql",
          page: {
            columns: ["total"],
            rows: [{ total: 99 }],
            has_more: false,
          },
        },
      },
    });

    expect(evaluate).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The metric result is incomplete.",
    );
    expect(screen.queryByText("99", { selector: "output" }))
      .not.toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "99" })).toBeInTheDocument();
  });

  it("rejects an inconsistent page that still has a continuation cursor", async () => {
    const evaluate = vi.spyOn(compositionApi, "evaluateMetricPage")
      .mockResolvedValue({ ok: true, value: 42 });
    const inconsistentPage = {
      summary: {
        ...sources.summary,
        page: {
          kind: "saved_sql" as const,
          page: {
            columns: ["total"],
            rows: [{ total: 42 }],
            has_more: false,
            next: "unexpected-next",
          },
        },
      },
    };
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={inconsistentPage}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    await waitFor(() =>
      expect(evaluate).toHaveBeenCalledWith({
        expected_result_type: "float",
        is_complete: false,
        row_count: 1,
        selected_column_count: 1,
        selected_value: 42,
      })
    );
  });

  it("offers a retry action when the shared metric evaluator fails", async () => {
    vi.spyOn(compositionApi, "evaluateMetricPage").mockRejectedValue(
      new Error("temporary evaluator failure"),
    );
    const onRetry = vi.fn();
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={sources}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={onRetry}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Metric result unavailable",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledWith("summary");
  });

  it("offers a retry when the metric source query fails", async () => {
    const onRetry = vi.fn();
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={{
          summary: {
            status: "error",
            cursorStack: [undefined],
            error: new Error("temporary query failure"),
          },
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={onRetry}
      />
    ));

    expect(await screen.findAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not load results.",
    );
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledWith("summary");
  });

  it("shows one loading indicator when multiple components share a source", () => {
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={{
          summary: { status: "loading", cursorStack: [undefined] },
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(screen.getAllByRole("status")).toHaveLength(1);
  });

  it("composition_renderer_delegates_to_source_native_presenters_without_client_aggregation", async () => {
    const delegatedPlan: CompositionResolvePlan = {
      composition_revision: { entry_id: "tool-1", revision_id: "revision-1" },
      sources: [
        {
          kind: "entry_query",
          source_id: "entries",
          source_schema_fingerprint: "fingerprint",
          request: {
            query: {
              scope: { kind: "form", form_id: "form-1" },
              filters: [],
              sort: [],
            },
            projection: {
              kind: "fields",
              fields: [{ kind: "property", field_id: 7 }, { kind: "created_at" }],
            },
            limit: 100,
          },
        },
        {
          kind: "saved_sql",
          source_id: "summary",
          source_schema_fingerprint: "fingerprint",
          request: {
            sql: "SELECT total",
            limit: 100,
            saved_sql: { id: "sql-1", revision_id: "sql-revision-1" },
          },
        },
      ],
      component_bindings: [
        {
          component_id: "entry-table",
          kind: "table",
          label: "Entries",
          source_id: "entries",
        },
        {
          component_id: "sql-table",
          kind: "table",
          label: "Totals",
          source_id: "summary",
        },
      ],
    };
    render(() => (
      <CompositionRenderer
        plan={delegatedPlan}
        sources={{
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
                  created_at_micros: 1_772_960_000_000_000,
                  updated_at_micros: 1_772_963_000_000_000,
                  properties: { purpose: "Travel" },
                  preview: "Travel entry",
                }],
                has_more: false,
              },
            },
          },
          summary: {
            status: "ready",
            cursorStack: [undefined],
            page: {
              kind: "saved_sql",
              page: {
                columns: ["total"],
                rows: [{ total: 42 }],
                has_more: false,
              },
            },
          },
        }}
        fieldNames={(_formId, fieldId) =>
          fieldId === 7 ? "purpose" : undefined}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    const entryTable = document.querySelector("table.result-table--entry");
    expect(entryTable).not.toBeNull();
    expect(
      within(entryTable as HTMLElement).getAllByRole("columnheader").map((
        header,
      ) => header.textContent),
    ).toEqual(["purpose", "Created"]);
    expect(
      within(entryTable as HTMLElement).getByText("Travel"),
    ).toBeInTheDocument();
    expect(
      entryTable?.querySelector('[data-entry-id="entry-1"]'),
    ).not.toBeNull();

    const sqlTable = document.querySelector("table.result-table--sql");
    expect(sqlTable).not.toBeNull();
    expect(
      within(sqlTable as HTMLElement).getByRole("columnheader", {
        name: "total",
      }),
    ).toBeInTheDocument();
    expect(
      within(sqlTable as HTMLElement).getByRole("cell", { name: "42" }),
    ).toBeInTheDocument();

    expect(screen.queryByRole("button")).toBeNull();
    expect(document.querySelector("output")).toBeNull();
  });
});
