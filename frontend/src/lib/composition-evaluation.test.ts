import { describe, expect, it } from "vitest";
import {
  affectedCompositionQueries,
  changedCompositionQueryIds,
  mergeCompositionEvaluation,
} from "./composition-evaluation";
import type { CompositionEvaluationResponse } from "./composition-api";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "./composition-variable-binding";

const query = (variable_id: string, ...dependencies: string[]) => ({
  variable_id,
  deferred_arguments: dependencies.map((variable_id) => ({
    argument_name: variable_id,
    variable_id,
    value_type: { scalar_type: "integer" as const, nullable: false },
  })),
  returns: {
    kind: "scalar" as const,
    value_type: { scalar_type: "integer" as const, nullable: false },
  },
  result_columns: [{
    name: "value",
    value_type: { scalar_type: "integer" as const, nullable: false },
  }],
  form_bindings: [],
  schema_fingerprint: "schema",
  query_semantic_fingerprint: `semantic-${variable_id}`,
  scalar_cardinality: "exactly_one_row" as const,
});

const response = (
  overrides: Partial<CompositionEvaluationResponse> = {},
): CompositionEvaluationResponse => ({
  evaluation_id: "evaluation",
  ok: true,
  composition_revision: { entry_id: "comp", revision_id: "rev-1" },
  publication_fingerprint: "publication-1",
  authorization_fingerprint: "auth-1",
  computed_query_ids: [],
  variable_values: {},
  queries: [
    query("rows"),
    query("count", "rows"),
    query("other"),
  ],
  component_bindings: [],
  dependencies: {
    variable_consumers: {
      day: { query_variable_ids: ["rows"] },
      rows: { query_variable_ids: ["count"] },
      other_day: { query_variable_ids: ["other"] },
    },
  },
  query_results: {},
  diagnostics: [],
  query_diagnostics: [],
  ...overrides,
});

const graph = (
  variables: CompositionGraphDraft["variables"],
): CompositionGraphDraft => ({
  ...createCompositionGraphDraft("Report"),
  variables,
});

describe("Composition graph evaluation reuse", () => {
  it("invalidates direct and downstream query previews for semantic draft edits", () => {
    const constant = {
      id: "threshold",
      kind: "constant" as const,
      value_type: { scalar_type: "integer" as const, nullable: false },
      value: { kind: "integer" as const, value: 1 },
    };
    const downstream = {
      id: "total",
      kind: "query" as const,
      returns: {
        kind: "scalar" as const,
        value_type: { scalar_type: "integer" as const, nullable: false },
      },
      query: {
        kind: "inline_sql" as const,
        sql: "SELECT $threshold",
        arguments: [{ name: "threshold", variable: "threshold" }],
      },
    };
    const count = {
      ...downstream,
      id: "count",
      query: {
        kind: "inline_sql" as const,
        sql: "SELECT $total",
        arguments: [{ name: "total", variable: "total" }],
      },
    };
    const base = graph([constant, downstream, count]);

    const constantChanged = graph([
      {
        ...constant,
        value: { kind: "integer", value: 2 },
      },
      downstream,
      count,
    ]);
    expect(changedCompositionQueryIds(base, constantChanged)).toEqual([
      "total",
      "count",
    ]);

    const rebound = graph([constant, {
      ...downstream,
      query: {
        ...downstream.query,
        arguments: [{ name: "threshold", variable: "replacement" }],
      },
    }, count]);
    expect(changedCompositionQueryIds(base, rebound)).toContain("total");
    expect(changedCompositionQueryIds(base, rebound)).toContain("count");

    const renamed = graph([{
      ...constant,
      id: "minimum",
    }, {
      ...downstream,
      query: {
        ...downstream.query,
        arguments: [{ name: "threshold", variable: "minimum" }],
      },
    }, count]);
    expect(changedCompositionQueryIds(base, renamed)).toContain("total");

    const removedQuery = graph([constant]);
    expect(changedCompositionQueryIds(base, removedQuery)).toContain("count");
    expect(changedCompositionQueryIds(base, removedQuery)).toContain("total");
  });

  it("requests only queries affected by a changed input and its dependents", () => {
    expect(
      affectedCompositionQueries(
        response().queries,
        response().dependencies,
        new Set(["day"]),
      ),
    ).toEqual(["rows", "count"]);
  });

  it("keeps unaffected query values only inside the same authority snapshot", () => {
    const previous = response({
      query_results: {
        rows: {
          value: { kind: "scalar", value: { kind: "integer", value: 1 } },
          has_more: false,
        },
        count: {
          value: { kind: "scalar", value: { kind: "integer", value: 2 } },
          has_more: false,
        },
        other: {
          value: { kind: "scalar", value: { kind: "integer", value: 3 } },
          has_more: false,
        },
      },
    });
    const next = response({
      query_results: {
        rows: {
          value: { kind: "scalar", value: { kind: "integer", value: 4 } },
          has_more: false,
        },
        count: {
          value: { kind: "scalar", value: { kind: "integer", value: 5 } },
          has_more: false,
        },
      },
      computed_query_ids: ["rows", "count"],
    });

    const merged = mergeCompositionEvaluation(previous, next, [
      "rows",
      "count",
    ]);
    expect(merged.query_results.rows.value).toEqual({
      kind: "scalar",
      value: { kind: "integer", value: 4 },
    });
    expect(merged.query_results.count.value).toEqual({
      kind: "scalar",
      value: { kind: "integer", value: 5 },
    });
    expect(merged.query_results.other.value).toEqual({
      kind: "scalar",
      value: { kind: "integer", value: 3 },
    });
  });

  it("retains unselected query descriptors for later independent input edits", () => {
    const previous = response();
    const partial = response({
      queries: [query("rows"), query("count", "rows")],
      computed_query_ids: ["rows", "count"],
    });

    const merged = mergeCompositionEvaluation(previous, partial, [
      "rows",
      "count",
    ]);

    expect(merged.queries.map((item) => item.variable_id)).toEqual([
      "rows",
      "count",
      "other",
    ]);
    expect(
      affectedCompositionQueries(
        merged.queries,
        merged.dependencies,
        new Set(["other_day"]),
      ),
    ).toEqual(["other"]);
  });

  it("accepts omitted empty diagnostics from a successful evaluation", () => {
    const previous = response({
      query_results: {
        other: {
          value: { kind: "scalar", value: { kind: "integer", value: 3 } },
          has_more: false,
        },
      },
    });
    const next = response({
      query_results: {
        rows: {
          value: { kind: "scalar", value: { kind: "integer", value: 4 } },
          has_more: false,
        },
      },
      computed_query_ids: ["rows"],
    });
    delete previous.diagnostics;
    delete previous.query_diagnostics;
    delete next.diagnostics;
    delete next.query_diagnostics;

    const merged = mergeCompositionEvaluation(previous, next, ["rows"]);
    expect(merged.query_results.rows).toBeDefined();
    expect(merged.query_results.other).toBeDefined();
    expect(merged.query_diagnostics).toEqual([]);
  });

  it.each(
    [
      ["authorization", { authorization_fingerprint: "auth-2" }],
      ["publication", { publication_fingerprint: "publication-2" }],
    ] as const,
  )("drops every retained value when %s changes", (_name, identity) => {
    const previous = response({
      query_results: {
        other: {
          value: { kind: "scalar", value: { kind: "integer", value: 3 } },
          has_more: false,
        },
      },
    });
    const next = response({
      ...identity,
      query_results: {
        rows: {
          value: { kind: "scalar", value: { kind: "integer", value: 4 } },
          has_more: false,
        },
      },
    });

    expect(mergeCompositionEvaluation(previous, next, ["rows"]).query_results)
      .toEqual(next.query_results);
  });

  it("retains unaffected results after a failed partial evaluation in the same snapshot", () => {
    const previous = response({
      query_results: {
        rows: {
          value: { kind: "scalar", value: { kind: "integer", value: 1 } },
          has_more: false,
          next: "old-rows-cursor",
        },
        count: {
          value: { kind: "scalar", value: { kind: "integer", value: 2 } },
          has_more: false,
        },
        other: {
          value: { kind: "scalar", value: { kind: "integer", value: 3 } },
          has_more: false,
          next: "unrelated-cursor",
        },
      },
    });
    const next = response({
      ok: false,
      computed_query_ids: ["rows", "count"],
      query_results: {},
      query_diagnostics: [
        { code: "sql_error", variable_id: "rows" },
        { code: "evaluation_aborted", variable_id: "count" },
      ],
    });

    const merged = mergeCompositionEvaluation(previous, next, [
      "rows",
      "count",
    ]);
    expect(merged.query_results.rows).toBeUndefined();
    expect(merged.query_results.count).toBeUndefined();
    expect(merged.query_results.other).toEqual(previous.query_results.other);
    expect(merged.query_diagnostics).toEqual(next.query_diagnostics);
  });
});
