import { describe, expect, it } from "vitest";
import { compositionQueryCountRequest } from "./composition-query-read";
import type { CompositionEvaluationResponse } from "./composition-api";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "./composition-variable-binding";

function fixture(): {
  draft: CompositionGraphDraft;
  response: CompositionEvaluationResponse;
} {
  const draft = createCompositionGraphDraft("Report");
  draft.variables = [
    {
      id: "search",
      kind: "constant",
      value_type: { scalar_type: "string", nullable: false },
      value: { kind: "string", value: "'; DROP TABLE ignored; --" },
    },
    {
      id: "floor",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: true },
      },
      query: {
        kind: "inline_sql",
        sql: "SELECT CAST(NULL AS BIGINT)",
        arguments: [],
      },
    },
    {
      id: "rows",
      kind: "query",
      returns: { kind: "table", columns: [] },
      query: {
        kind: "inline_sql",
        sql:
          'SELECT * FROM "Original name" WHERE name = $text AND value > $minimum',
        arguments: [{ name: "text", variable: "search" }, {
          name: "minimum",
          variable: "floor",
        }],
      },
    },
  ];
  const response = {
    evaluation_id: "run",
    ok: true,
    computed_query_ids: ["floor", "rows"],
    variable_values: { search: draft.variables[0].value },
    queries: [{
      variable_id: "rows",
      portable_sql:
        "SELECT * FROM form_immutable WHERE field_100 = $text AND field_101 > $minimum",
      returns: { kind: "table", columns: [] },
    }],
    query_results: {
      floor: {
        value: { kind: "scalar", value: { kind: "null" } },
        has_more: false,
      },
      rows: {
        value: { kind: "table", columns: [], rows: [] },
        has_more: false,
      },
    },
    component_bindings: [],
    dependencies: {},
  } as unknown as CompositionEvaluationResponse;
  return { draft, response };
}

describe("Composition explicit query count", () => {
  it("uses Rust's immutable Form SQL and validated scalar dependencies without interpolation", () => {
    const { draft, response } = fixture();
    const request = compositionQueryCountRequest(draft, response, "rows");
    expect(request).toEqual({
      sql: response.queries[0].portable_sql,
      parameters: { text: "'; DROP TABLE ignored; --", minimum: null },
      parameter_types: { text: "string", minimum: "integer" },
    });
    expect(request.sql).not.toContain("Original name");
    expect(request.sql).not.toContain("DROP TABLE");
    expect(request).not.toHaveProperty("limit");
  });

  it("keeps an exact Saved SQL revision instead of using the moving head or copied SQL", () => {
    const { draft, response } = fixture();
    const node = draft.variables[2];
    if (node.kind !== "query") throw new Error("query fixture");
    node.query = {
      kind: "saved_sql",
      entry_id: "saved",
      revision_id: "pinned",
      arguments: node.query.arguments,
    };
    expect(compositionQueryCountRequest(draft, response, "rows")).toMatchObject(
      { sql: "", saved_sql: { id: "saved", revision_id: "pinned" } },
    );
  });

  it("refuses a missing result or an unvalidated scalar dependency", () => {
    const { draft, response } = fixture();
    delete response.query_results.floor;
    expect(() => compositionQueryCountRequest(draft, response, "rows")).toThrow(
      "validated SQL scalar",
    );
    delete response.query_results.rows;
    expect(() => compositionQueryCountRequest(draft, response, "rows")).toThrow(
      "validated table",
    );
  });
});
