import "@testing-library/jest-dom/vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CompositionRenderer } from "./CompositionRenderer";
import { compositionApi } from "~/lib/composition-api";
import type {
  CompositionMetricQueryResponse,
  CompositionResolvePlan,
} from "~/lib/composition-api";

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
    expected_result_type: "integer",
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

  it("queries an exact integer metric through the bounded server operation", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "sql_scalar",
        source_kind: "saved_sql",
        result_type: "integer",
        value: "9007199254740993",
      },
    });
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: { month: "2026-10" },
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("9007199254740993", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledWith(
      "space-1",
      {
        target: {
          kind: "saved_revision",
          composition_id: "tool-1",
          revision_id: "revision-1",
        },
        component_id: "total",
        parameters: { month: "2026-10" },
      },
      expect.any(AbortSignal),
    );
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("queries draft metrics and verifies the preview fingerprint", async () => {
    const yaml = "format: ugoite.composition\n";
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      draft_fingerprint: "draft-fingerprint",
      result: {
        component_id: "total",
        operation: "sum",
        source_kind: "entry_query",
        result_type: "integer",
        value: "42",
      },
    });
    render(() => (
      <CompositionRenderer
        plan={{ ...plan, component_bindings: [plan.component_bindings[0]] }}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: { kind: "draft", yaml },
          draftFingerprint: "draft-fingerprint",
          parameters: { year: 2026 },
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledWith(
      "space-1",
      {
        target: { kind: "draft", yaml },
        component_id: "total",
        parameters: { year: 2026 },
      },
      expect.any(AbortSignal),
    );
  });

  it("rejects a draft metric result for a different preview fingerprint", async () => {
    vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      draft_fingerprint: "stale-fingerprint",
      result: {
        component_id: "total",
        operation: "sum",
        source_kind: "entry_query",
        result_type: "integer",
        value: "42",
      },
    });
    render(() => (
      <CompositionRenderer
        plan={{ ...plan, component_bindings: [plan.component_bindings[0]] }}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: { kind: "draft", yaml: "format: ugoite.composition\n" },
          draftFingerprint: "current-fingerprint",
          parameters: {},
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Metric result format is invalid",
    );
    expect(screen.queryByText("42", { selector: "output" }))
      .not.toBeInTheDocument();
  });

  it("does not requery metrics when the parent recreates the same context", async () => {
    const [parentState, setParentState] = createSignal(1);
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
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    render(() => (
      <>
        <span data-testid="parent-state">{parentState()}</span>
        <CompositionRenderer
          plan={metricPlan}
          sources={{}}
          metricQueryContext={(() => {
            parentState();
            return {
              spaceId: "space-1",
              target: {
                kind: "saved_revision" as const,
                composition_id: "tool-1",
                revision_id: "revision-1",
              },
              parameters: { month: "2026-10" },
            };
          })()}
          onNext={() => {}}
          onPrevious={() => {}}
          onRetry={() => {}}
        />
      </>
    ));

    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    setParentState(2);
    expect(await screen.findByTestId("parent-state")).toHaveTextContent("2");
    expect(query).toHaveBeenCalledOnce();
  });

  it("shows a successful SQL NULL separately from a numeric zero", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "sql_scalar",
        source_kind: "saved_sql",
        result_type: "integer",
        value: null,
      },
    });
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: {},
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("No data", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledOnce();
  });

  it("renders a numeric zero instead of treating it as missing data", async () => {
    vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "count_rows",
        source_kind: "entry_query",
        result_type: "integer",
        value: "0",
      },
    });
    render(() => (
      <CompositionRenderer
        plan={{ ...plan, component_bindings: [plan.component_bindings[0]] }}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: {},
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("0", { selector: "output" }))
      .toBeInTheDocument();
    expect(screen.queryByText("No data")).not.toBeInTheDocument();
  });

  it("applies only explicit metric formatting without rounding or losing decimal scale", async () => {
    const [parameters, setParameters] = createSignal({ month: "2026-10" });
    const query = vi.spyOn(compositionApi, "queryMetric")
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
          operation: "sql_scalar",
          source_kind: "saved_sql",
          result_type: "decimal",
          value: "12.3",
        },
      })
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
          operation: "sql_scalar",
          source_kind: "saved_sql",
          result_type: "decimal",
          value: "12.345",
        },
      });
    const metricPlan = {
      ...plan,
      component_bindings: [{
        ...plan.component_bindings[0],
        expected_result_type: "decimal" as const,
        presentation: {
          kind: "number" as const,
          unit: " USD ",
          fraction_digits: 2,
        },
      }],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: parameters(),
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("12.30 USD", { selector: "output" }))
      .toBeInTheDocument();
    setParameters({ month: "2026-11" });
    expect(await screen.findByText("12.345 USD", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(2);
  });

  it("formats finite float values written in scientific notation", async () => {
    vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "avg",
        source_kind: "entry_query",
        result_type: "float",
        value: 1e-7,
      },
    });
    const metricPlan = {
      ...plan,
      component_bindings: [{
        ...plan.component_bindings[0],
        expected_result_type: "float" as const,
        presentation: {
          kind: "number" as const,
          unit: "kg",
          fraction_digits: 8,
        },
      }],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: {},
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("0.00000010 kg", { selector: "output" }))
      .toBeInTheDocument();
  });

  it("preserves string metric values that resemble scientific notation", async () => {
    vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "sql_scalar",
        source_kind: "saved_sql",
        result_type: "string",
        value: "1e3",
      },
    });
    const metricPlan = {
      ...plan,
      component_bindings: [{
        ...plan.component_bindings[0],
        expected_result_type: "string" as const,
      }],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: {},
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("1e3", { selector: "output" }))
      .toBeInTheDocument();
    expect(screen.queryByText("1000", { selector: "output" }))
      .not.toBeInTheDocument();
  });

  it("rejects malformed successful metric responses instead of rendering their values", async () => {
    vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
        operation: "sql_scalar",
        source_kind: "saved_sql",
        result_type: "integer",
        value: { internal: "not a scalar" },
      },
    } as unknown as CompositionMetricQueryResponse);
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: {},
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Metric result format is invalid",
    );
    expect(screen.queryByText("not a scalar")).not.toBeInTheDocument();
    expect(document.querySelector("output.compositionMetric")).toBeNull();
  });

  it("sends a non-scalar Saved SQL metric directly to edit", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: false,
      diagnostics: [{ code: "metric_scalar_expected_one_row" }],
    });
    const onEdit = vi.fn();
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: {},
          onEdit,
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The metric source must return exactly one row.",
    );
    expect(query).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: "Retry" })).not
      .toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledOnce();
  });

  const expectDirectEditForScalarDiagnostic = async (
    code: string,
    message: string,
  ) => {
    vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: false,
      diagnostics: [{ code }],
    });
    const onEdit = vi.fn();
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: {},
          onEdit,
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledOnce();
  };

  it("offers direct edit for an empty Saved SQL metric result", async () => {
    await expectDirectEditForScalarDiagnostic(
      "metric_result_empty",
      "The metric returned no value.",
    );
  });

  it("offers direct edit for a multi-row Saved SQL metric result", async () => {
    await expectDirectEditForScalarDiagnostic(
      "metric_result_multiple_rows",
      "The metric returned more than one row.",
    );
  });

  it("retries a metric query and refreshes it when parameters change", async () => {
    const [parameters, setParameters] = createSignal({ month: "2026-10" });
    const query = vi.spyOn(compositionApi, "queryMetric")
      .mockRejectedValueOnce(new Error("temporary query failure"))
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
          operation: "sql_scalar",
          source_kind: "saved_sql",
          result_type: "decimal",
          value: "12.300",
        },
      })
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
          operation: "sql_scalar",
          source_kind: "saved_sql",
          result_type: "decimal",
          value: "15.600",
        },
      });
    const metricPlan = {
      ...plan,
      component_bindings: [{
        ...plan.component_bindings[0],
        expected_result_type: "decimal" as const,
      }],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: parameters(),
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Metric result unavailable",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("12.300", { selector: "output" }))
      .toBeInTheDocument();
    setParameters({ month: "2026-11" });
    expect(await screen.findByText("15.600", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenNthCalledWith(
      1,
      "space-1",
      expect.objectContaining({ parameters: { month: "2026-10" } }),
      expect.any(AbortSignal),
    );
    expect(query).toHaveBeenNthCalledWith(
      3,
      "space-1",
      expect.objectContaining({ parameters: { month: "2026-11" } }),
      expect.any(AbortSignal),
    );
  });

  it("discards a slow result after the metric parameters change", async () => {
    const [parameters, setParameters] = createSignal({ month: "2026-10" });
    let resolveOld!: (result: CompositionMetricQueryResponse) => void;
    let resolveCurrent!: (result: CompositionMetricQueryResponse) => void;
    const query = vi.spyOn(compositionApi, "queryMetric")
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveOld = resolve;
        })
      )
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveCurrent = resolve;
        })
      );
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    render(() => (
      <CompositionRenderer
        plan={metricPlan}
        sources={{}}
        metricQueryContext={{
          spaceId: "space-1",
          target: {
            kind: "saved_revision",
            composition_id: "tool-1",
            revision_id: "revision-1",
          },
          parameters: parameters(),
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));
    await waitFor(() => expect(query).toHaveBeenCalledTimes(1));
    setParameters({ month: "2026-11" });
    await waitFor(() => expect(query).toHaveBeenCalledTimes(2));

    resolveCurrent({
      ok: true,
      result: {
        component_id: "total",
        operation: "count_rows",
        source_kind: "entry_query",
        result_type: "integer",
        value: "2",
      },
    });
    expect(await screen.findByText("2", { selector: "output" }))
      .toBeInTheDocument();
    resolveOld({
      ok: true,
      result: {
        component_id: "total",
        operation: "count_rows",
        source_kind: "entry_query",
        result_type: "integer",
        value: "1",
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.getByText("2", { selector: "output" })).toBeInTheDocument();
    expect(screen.queryByText("1", { selector: "output" })).not
      .toBeInTheDocument();
  });

  it("does not evaluate a metric from a paginated source result", () => {
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={sources}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(screen.getByRole("heading", { name: "Total" })).toBeInTheDocument();
    expect(screen.getByRole("table", { name: "Details" })).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(document.querySelector("output.compositionMetric")).toBeNull();
  });

  it("aligns an all-null Saved SQL column from its server result schema", async () => {
    const nullValueSources = {
      summary: {
        ...sources.summary,
        page: {
          kind: "saved_sql" as const,
          page: {
            columns: ["total"],
            result_schema: [{ name: "total", type: "float" as const }],
            rows: [{ total: null }],
            has_more: false,
          },
        },
      },
    };
    render(() => (
      <CompositionRenderer
        plan={plan}
        sources={nullValueSources}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(screen.getByRole("table", { name: "Details" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Details" })).toHaveAttribute(
      "tabindex",
      "0",
    );
    expect(screen.getByRole("columnheader", { name: "total" })).toHaveClass(
      "result-table-header--numeric",
    );
    expect(screen.getByRole("cell", { name: "—" })).toHaveClass(
      "result-table-cell--numeric",
    );
  });

  it("offers a retry when a table source query fails", async () => {
    const onRetry = vi.fn();
    render(() => (
      <CompositionRenderer
        plan={{ ...plan, component_bindings: [plan.component_bindings[1]] }}
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

  it("renders text bindings from joined declarations without source requests", async () => {
    const textPlan: CompositionResolvePlan = {
      composition_revision: { entry_id: "tool-1", revision_id: "revision-1" },
      sources: [],
      component_bindings: [{
        component_id: "note",
        kind: "text",
        label: "Note",
      }],
    };
    render(() => (
      <CompositionRenderer
        plan={textPlan}
        sources={{}}
        texts={{ note: { text: "Settled monthly.", style: "body" } }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(screen.getByText("Settled monthly.")).toHaveClass("flowText--body");
  });

  it("skips text bindings when the caller has no component declarations", () => {
    const textPlan: CompositionResolvePlan = {
      composition_revision: { entry_id: "tool-1", revision_id: "revision-1" },
      sources: [],
      component_bindings: [{
        component_id: "note",
        kind: "text",
        label: "Note",
      }],
    };
    const { container } = render(() => (
      <CompositionRenderer
        plan={textPlan}
        sources={{}}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(container.querySelector(".compositionRenderer")?.textContent)
      .toBe("");
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
              fields: [{ kind: "property", field_id: 7 }, {
                kind: "created_at",
              }],
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
        fieldNames={(_formId, fieldId) => fieldId === 7 ? "purpose" : undefined}
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
