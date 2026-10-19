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
import {
  compositionDiagnosticMessage,
  CompositionRenderer,
} from "./CompositionRenderer";
import { compositionApi } from "~/lib/composition-api";
import { setLocale } from "~/lib/i18n";
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
    expected_result: [{ name: "total", type: "integer" }],
    request: {
      sql: "SELECT total",
      limit: 1,
      saved_sql: { id: "sql-1", revision_id: "sql-revision-1" },
    },
  }],
  component_bindings: [{
    component_id: "total",
    kind: "metric",
    source_id: "summary",
    result_property_key: "total",
    expected_result_type: "integer",
  }, {
    component_id: "rows",
    kind: "table",
    source_id: "summary",
  }],
  dependencies: {
    variable_consumers: {
      month: { source_ids: ["summary"] },
      year: { source_ids: ["summary"] },
    },
  },
};

const sources = {
  summary: {
    status: "ready" as const,
    cursorStack: [undefined],
    page: {
      kind: "sql" as const,
      page: {
        columns: ["total"],
        rows: [{ total: 42 }],
        has_more: false,
      },
    },
  },
};

describe("CompositionRenderer", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    setLocale("en");
  });

  it("keeps stable variable IDs in diagnostics in either locale", () => {
    const unknown = compositionDiagnosticMessage({
      code: "variable_unknown",
      variable_id: "internal_key",
    });
    expect(unknown).toBe("Unknown variable: internal_key.");

    setLocale("ja");
    const missing = compositionDiagnosticMessage(
      { code: "variable_missing", variable_id: "month" },
      [{ id: "month" }],
    );
    expect(missing).toBe("month の値を入力してください。");
  });

  it("queries an exact integer metric through the bounded server operation", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
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
          variables: { month: "2026-10" },
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
        variables: { month: "2026-10" },
      },
      expect.any(AbortSignal),
    );
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("keeps an unaffected metric ready and waits for changed inputs while another variable blocks resolution", async () => {
    const [variables, setVariables] = createSignal({
      month: "2026-03",
      status: "active",
    });
    const [invalidVariables, setInvalidVariables] = createSignal<string[]>([]);
    const query = vi.spyOn(compositionApi, "queryMetric").mockImplementation(
      async (_spaceId, request) => ({
        ok: true,
        result: {
          component_id: "total",
          result_type: "integer",
          value: request.variables.month === "2026-03" ? "42" : "84",
        },
      }),
    );
    const metricPlan = {
      ...plan,
      component_bindings: [plan.component_bindings[0]],
    };
    const dependencies = {
      variable_consumers: {
        month: { source_ids: ["summary"] },
        status: { source_ids: ["other"] },
      },
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
          variables: variables(),
          invalidVariables: invalidVariables(),
          dependencies,
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("42", { selector: "output" }))
      .toBeInTheDocument();
    setInvalidVariables(["status"]);
    expect(screen.getByText("42", { selector: "output" })).toBeInTheDocument();
    expect(query).toHaveBeenCalledOnce();

    setVariables({ month: "2026-04", status: "active" });
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Waiting for input…",
    );
    expect(screen.queryByText("42", { selector: "output" }))
      .not.toBeInTheDocument();
    expect(query).toHaveBeenCalledOnce();

    setInvalidVariables([]);
    expect(await screen.findByText("84", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(2);
  });

  it("renders an accessible waiting state for a blocked Saved SQL table", () => {
    setLocale("ja");
    const tablePlan = {
      ...plan,
      component_bindings: [plan.component_bindings[1]],
    };
    render(() => (
      <CompositionRenderer
        plan={tablePlan}
        sources={{ summary: { status: "waiting", cursorStack: [undefined] } }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(screen.getByRole("status")).toHaveTextContent("入力待ち…");
    expect(screen.queryByText("結果を読み込み中…"))
      .not.toBeInTheDocument();
  });

  it("invalidates a metric when an unset dependency becomes invalid without changing its null value", async () => {
    const [invalidVariables, setInvalidVariables] = createSignal<string[]>([]);
    let resolveQuery!: (response: CompositionMetricQueryResponse) => void;
    const query = vi.spyOn(compositionApi, "queryMetric").mockImplementation(
      () => new Promise((resolve) => resolveQuery = resolve),
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
          variables: { month: null },
          invalidVariables: invalidVariables(),
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    await waitFor(() => expect(query).toHaveBeenCalledOnce());
    const signal = query.mock.calls[0][2];
    setInvalidVariables(["month"]);

    expect(await screen.findByText("Enter a valid value.")).toBeInTheDocument();
    expect(signal.aborted).toBe(true);
    resolveQuery({
      ok: true,
      result: {
        component_id: "total",
        result_type: "integer",
        value: "42",
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(screen.queryByText("42", { selector: "output" }))
      .not.toBeInTheDocument();
  });

  it("invalidates a metric when its dependency index becomes incomplete", async () => {
    const [dependencies, setDependencies] = createSignal<
      CompositionResolvePlan["dependencies"]
    >(plan.dependencies);
    let resolveQuery!: (response: CompositionMetricQueryResponse) => void;
    const query = vi.spyOn(compositionApi, "queryMetric").mockImplementation(
      () => new Promise((resolve) => resolveQuery = resolve),
    );
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
          variables: { month: "2026-10" },
          dependencies: dependencies(),
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    await waitFor(() => expect(query).toHaveBeenCalledOnce());
    const signal = query.mock.calls[0][2];
    setDependencies({ variable_consumers: {} });

    expect(await screen.findByText("Enter a valid value.")).toBeInTheDocument();
    expect(signal.aborted).toBe(true);
    resolveQuery({
      ok: true,
      result: {
        component_id: "total",
        result_type: "integer",
        value: "42",
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByText("42", { selector: "output" }))
      .not.toBeInTheDocument();
  });

  it("queries draft metrics and verifies the preview fingerprint", async () => {
    const yaml = "format: ugoite.composition\n";
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      draft_fingerprint: "draft-fingerprint",
      result: {
        component_id: "total",
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
          variables: { year: 2026 },
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
        variables: { year: 2026 },
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
          variables: {},
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
              variables: { month: "2026-10" },
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
          variables: {},
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
          variables: {},
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
    const [variables, setVariables] = createSignal({ month: "2026-10" });
    const query = vi.spyOn(compositionApi, "queryMetric")
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
          result_type: "decimal",
          value: "12.3",
        },
      })
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
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
          variables: variables(),
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByText("12.30 USD", { selector: "output" }))
      .toBeInTheDocument();
    setVariables({ month: "2026-11" });
    expect(await screen.findByText("12.345 USD", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenCalledTimes(2);
  });

  it("formats finite float values written in scientific notation", async () => {
    vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: true,
      result: {
        component_id: "total",
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
          variables: {},
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
          variables: {},
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
          variables: {},
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

  it("sends a multi-row Saved SQL metric directly to edit", async () => {
    const query = vi.spyOn(compositionApi, "queryMetric").mockResolvedValue({
      ok: false,
      diagnostics: [{ code: "metric_scalar_multiple_rows" }],
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
          variables: {},
          onEdit,
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The metric returned more than one row.",
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
          variables: {},
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

  it("offers direct edit when a Saved SQL metric returns no rows", async () => {
    await expectDirectEditForScalarDiagnostic(
      "metric_scalar_no_rows",
      "The metric returned no rows.",
    );
  });

  it("retries a metric query and refreshes it when variables change", async () => {
    const [variables, setVariables] = createSignal({ month: "2026-10" });
    const query = vi.spyOn(compositionApi, "queryMetric")
      .mockRejectedValueOnce(new Error("temporary query failure"))
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
          result_type: "decimal",
          value: "12.300",
        },
      })
      .mockResolvedValueOnce({
        ok: true,
        result: {
          component_id: "total",
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
          variables: variables(),
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
    setVariables({ month: "2026-11" });
    expect(await screen.findByText("15.600", { selector: "output" }))
      .toBeInTheDocument();
    expect(query).toHaveBeenNthCalledWith(
      1,
      "space-1",
      expect.objectContaining({ variables: { month: "2026-10" } }),
      expect.any(AbortSignal),
    );
    expect(query).toHaveBeenNthCalledWith(
      3,
      "space-1",
      expect.objectContaining({ variables: { month: "2026-11" } }),
      expect.any(AbortSignal),
    );
  });

  it("discards a slow result after the metric variables change", async () => {
    const [variables, setVariables] = createSignal({ month: "2026-10" });
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
          variables: variables(),
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));
    await waitFor(() => expect(query).toHaveBeenCalledTimes(1));
    setVariables({ month: "2026-11" });
    await waitFor(() => expect(query).toHaveBeenCalledTimes(2));

    resolveCurrent({
      ok: true,
      result: {
        component_id: "total",
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

    expect(screen.getByRole("heading", { name: "total" })).toBeInTheDocument();
    expect(document.querySelector("table.result-table--sql")).not.toBeNull();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(document.querySelector("output.compositionMetric")).toBeNull();
  });

  it("aligns an all-null Saved SQL column from its server result schema", async () => {
    const nullValueSources = {
      summary: {
        ...sources.summary,
        page: {
          kind: "sql" as const,
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

    expect(document.querySelector("table.result-table--sql")).not.toBeNull();
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
          kind: "inline_sql",
          source_id: "expenses",
          source_schema_fingerprint: "fingerprint",
          form_bindings: [{ relation: "expenses", form_id: "form-1" }],
          expected_result: [{ name: "purpose", type: "string" }],
          request: {
            sql: 'SELECT "Purpose" AS purpose FROM "expenses"',
            parameters: {},
            parameter_types: {},
            limit: 100,
          },
        },
        {
          kind: "saved_sql",
          source_id: "summary",
          source_schema_fingerprint: "fingerprint",
          expected_result: [{ name: "total", type: "integer" }],
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
          source_id: "expenses",
        },
        {
          component_id: "sql-table",
          kind: "table",
          source_id: "summary",
        },
      ],
    };
    render(() => (
      <CompositionRenderer
        plan={delegatedPlan}
        sources={{
          expenses: {
            status: "ready",
            cursorStack: [undefined],
            page: {
              kind: "sql",
              page: {
                columns: ["purpose"],
                result_schema: [{ name: "purpose", type: "string" }],
                rows: [{ purpose: "Travel" }],
                has_more: false,
              },
            },
          },
          summary: {
            status: "ready",
            cursorStack: [undefined],
            page: {
              kind: "sql",
              page: {
                columns: ["total"],
                rows: [{ total: 42 }],
                has_more: false,
              },
            },
          },
        }}
        onNext={() => {}}
        onPrevious={() => {}}
        onRetry={() => {}}
      />
    ));

    const sqlTables = document.querySelectorAll("table.result-table--sql");
    expect(sqlTables).toHaveLength(2);
    expect(
      within(sqlTables[0] as HTMLElement).getByRole("columnheader", {
        name: "purpose",
      }),
    ).toBeInTheDocument();
    expect(
      within(sqlTables[1] as HTMLElement).getByRole("columnheader", {
        name: "total",
      }),
    ).toBeInTheDocument();
    expect(
      within(sqlTables[1] as HTMLElement).getByRole("cell", { name: "42" }),
    ).toBeInTheDocument();

    expect(screen.queryByRole("button")).toBeNull();
    expect(document.querySelector("output")).toBeNull();
  });
});
