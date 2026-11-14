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
        form_bindings: [{
          relation: "Original name",
          form_id: "01234567-89ab-cdef-0123-456789abcdef",
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
      form_bindings: [{
        relation: "Original name",
        form_id: "01234567-89ab-cdef-0123-456789abcdef",
      }],
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
        count_snapshot: "v1.opaque-count-coordinate",
      },
    },
    component_bindings: [],
    dependencies: {},
  } as unknown as CompositionEvaluationResponse;
  return { draft, response };
}

describe("Composition explicit query count", () => {
  it("uses the resolved ID-bound SQL and validated scalar dependencies without interpolation", () => {
    const { draft, response } = fixture();
    const request = compositionQueryCountRequest(draft, response, "rows");
    const rows = draft.variables.find((entry) => entry.id === "rows");
    if (rows?.kind !== "query") throw new Error("query fixture");
    expect(request).toEqual({
      sql: response.queries[0].portable_sql,
      parameters: { text: "'; DROP TABLE ignored; --", minimum: null },
      parameter_types: { text: "string", minimum: "integer" },
      composition_count_snapshot: "v1.opaque-count-coordinate",
    });
    expect(request.sql).not.toContain("DROP TABLE");
    expect(request).not.toHaveProperty("limit");
  });

  it("requires a current resolved Table query for a count", () => {
    const { draft, response } = fixture();
    response.queries = [];
    expect(() => compositionQueryCountRequest(draft, response, "rows"))
      .toThrow("A current validated table query is required");
  });

  it("keeps opaque Form relation SQL unchanged for its exact count snapshot", () => {
    const { draft, response } = fixture();
    const rows = draft.variables.find((entry) => entry.id === "rows");
    if (rows?.kind !== "query") throw new Error("query fixture");
    rows.query.sql = 'SELECT * FROM "form_0123456789abcdef0123456789abcdef"';
    rows.query.form_bindings = [{
      relation: "form_0123456789abcdef0123456789abcdef",
      form_id: "01234567-89ab-cdef-0123-456789abcdef",
    }];
    response.queries[0].form_bindings = rows.query.form_bindings;
    response.queries[0].portable_sql = rows.query.sql;

    const request = compositionQueryCountRequest(draft, response, "rows");
    expect(request.sql).toBe(rows.query.sql);
    expect(request).not.toHaveProperty("form_binding_guards");
  });

  it("uses the resolver's stable relation when a Form name resembles an ID", () => {
    const { draft, response } = fixture();
    const rows = draft.variables.find((entry) => entry.id === "rows");
    if (rows?.kind !== "query") throw new Error("query fixture");
    const relation = "form_0123456789abcdef0123456789abcdef";
    const formId = "ffffffff-ffff-4fff-8fff-ffffffffffff";
    rows.query.sql = `SELECT * FROM "${relation}"`;
    response.queries[0].form_bindings = [{ relation, form_id: formId }];
    response.queries[0].portable_sql =
      'SELECT * FROM form_ffffffffffffffffffffffffffffffff AS "form_0123456789abcdef0123456789abcdef"';

    expect(compositionQueryCountRequest(draft, response, "rows"))
      .toMatchObject({
        sql: response.queries[0].portable_sql,
      });
    expect(compositionQueryCountRequest(draft, response, "rows"))
      .not.toHaveProperty("form_binding_guards");
  });

  it("preserves a valid zero-valued scalar argument for explicit Count", () => {
    const { draft, response } = fixture();
    draft.variables[1] = {
      id: "floor",
      kind: "input",
      value_type: { scalar_type: "float", nullable: false },
      required: true,
      default: { kind: "float", value: 0 },
    };
    delete response.query_results.floor;
    response.variable_values.floor = 0;
    expect(compositionQueryCountRequest(draft, response, "rows")).toMatchObject(
      {
        parameters: { minimum: 0 },
        parameter_types: { minimum: "float" },
      },
    );
  });

  it("preserves prototype-shaped SQL parameter names across JSON transport", () => {
    // Deno omits this legacy setter, while browsers retain it.
    const original = Object.getOwnPropertyDescriptor(
      Object.prototype,
      "__proto__",
    );
    if (!original) {
      Object.defineProperty(Object.prototype, "__proto__", {
        configurable: true,
        set(this: object, value: unknown) {
          if (
            value === null || typeof value === "object" ||
            typeof value === "function"
          ) {
            Object.setPrototypeOf(this, value);
          }
        },
      });
    }
    try {
      const { draft, response } = fixture();
      const node = draft.variables[2];
      if (node.kind !== "query") throw new Error("query fixture");
      node.query.arguments = [{ name: "__proto__", variable: "floor" }];
      response.queries[0].portable_sql = "SELECT $__proto__";
      const wire = JSON.parse(JSON.stringify(
        compositionQueryCountRequest(draft, response, "rows"),
      ));
      expect(Object.hasOwn(wire.parameters, "__proto__")).toBe(true);
      expect(wire.parameters["__proto__"]).toBeNull();
      expect(Object.hasOwn(wire.parameter_types, "__proto__")).toBe(true);
      expect(wire.parameter_types["__proto__"]).toBe("integer");
    } finally {
      if (!original) Reflect.deleteProperty(Object.prototype, "__proto__");
    }
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
