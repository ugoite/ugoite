// REQ-FE-036: SQL Query Editor
import {
  buildSqlSchema,
  buildSqlStarterQuery,
  normalizeSqlVariables,
  buildSqlAggregateQuery,
  sqlAggregateFieldOptions,
  sqlFilterFieldOptions,
  sqlLintDiagnostics,
} from "./sql";
import type { Form } from "./types";

describe("sql helpers", () => {
  it("should flag missing select clause", () => {
    const diagnostics = sqlLintDiagnostics(
      "FROM form_00000000000000000000000000000001",
    );
    expect(diagnostics.some((diag) => diag.message.includes("SELECT"))).toBe(
      true,
    );
  });

  it("should accept valid select queries", () => {
    const diagnostics = sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001" LIMIT 10',
    );
    expect(diagnostics).toHaveLength(0);
  });

  it("accepts read-only SQL with numeric aggregate expressions", () => {
    const diagnostics = sqlLintDiagnostics(
      'SELECT COUNT(*) AS total, SUM(amount) AS sum, AVG(amount) AS average, MIN(amount) AS minimum, MAX(amount) AS maximum FROM "Expenses" WHERE amount IS NOT NULL',
    );

    expect(diagnostics).toEqual([]);
  });

  it("uses the shared SQL rules for linting", () => {
    const diagnostics = sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001" LIMIT invalid',
    );
    expect(diagnostics.map((diagnostic) => diagnostic.message)).toContain(
      "LIMIT value should be a number or parameter",
    );
  });

  it("reports the read-only policy separately from server syntax lint", () => {
    expect(sqlLintDiagnostics("DROP TABLE entries").map((diag) => diag.message))
      .toEqual([
        "Read-only query must start with SELECT",
        "Read-only query must include FROM",
      ]);
  });

  it("classifies read-only policy from the top-level statement form", () => {
    expect(
      sqlLintDiagnostics("INSERT INTO entries SELECT * FROM source").map((
        diagnostic,
      ) => diagnostic.message),
    ).toEqual(["Read-only query must start with SELECT"]);

    for (
      const query of [
        "INSERT INTO entries SELECT * FROM source",
        "UPDATE entries SET title = 'changed'",
        "DELETE FROM entries",
        "CREATE TABLE entries (title TEXT)",
        "ALTER TABLE entries ADD COLUMN title TEXT",
        "DROP TABLE entries",
      ]
    ) {
      expect(
        sqlLintDiagnostics(query).some((diagnostic) =>
          diagnostic.message === "Read-only query must start with SELECT"
        ),
        query,
      ).toBe(true);
    }
  });

  it("does not treat nested FROM clauses as the outer query's FROM", () => {
    expect(
      sqlLintDiagnostics("SELECT (SELECT 1 FROM entries)").some((diagnostic) =>
        diagnostic.message === "Read-only query must include FROM"
      ),
    ).toBe(true);
  });

  it("accepts a SELECT whose top-level form is introduced by a CTE", () => {
    expect(sqlLintDiagnostics("WITH rows AS (SELECT 1) SELECT * FROM rows"))
      .toEqual(
        [],
      );
  });

  it("does not treat a CTE followed by DML as a read-only query", () => {
    expect(
      sqlLintDiagnostics(
        "WITH source AS (SELECT * FROM entries) INSERT INTO entries SELECT * FROM source",
      ).some((diagnostic) =>
        diagnostic.message === "Read-only query must start with SELECT"
      ),
    ).toBe(true);
  });

  it("builds a starter query with the stateless SQL query total order", () => {
    expect(buildSqlStarterQuery("form_entry")).toBe(
      'SELECT * FROM "form_entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50',
    );
  });

  it.each([
    ["count-rows", "", 'SELECT COUNT(*) AS "value" FROM "Orders"'],
    ["count-values", "Amount", 'SELECT COUNT("field_amount") AS "value" FROM "Orders"'],
    ["sum", "Amount", 'SELECT SUM("field_amount") AS "value" FROM "Orders"'],
    ["average", "Amount", 'SELECT AVG("field_amount") AS "value" FROM "Orders"'],
    ["minimum", "Amount", 'SELECT MIN("field_amount") AS "value" FROM "Orders"'],
    ["maximum", "Amount", 'SELECT MAX("field_amount") AS "value" FROM "Orders"'],
  ] as const)("builds the %s aggregate query", (operation, fieldKey, expected) => {
    expect(buildSqlAggregateQuery(aggregateForm, operation, fieldKey).sql)
      .toBe(expected);
  });

  it("quotes Form and SQL column identifiers and keeps filter values parameterized", () => {
    const quotedForm = {
      ...aggregateForm,
      name: 'Orders "Daily"',
      fields: {
        ...aggregateForm.fields,
        Amount: {
          ...aggregateForm.fields.Amount,
          sql_column: 'field "amount"',
        },
      },
    };
    expect(buildSqlAggregateQuery(quotedForm, "sum", "Amount", "Category"))
      .toEqual({
        sql:
          'SELECT SUM("field ""amount""") AS "value" FROM "Orders ""Daily""" WHERE "field_category" = {{filter_value}}',
        parameter: {
          name: "filter_value",
          type: "string",
          description: "Category",
        },
      });
  });

  it("limits aggregate and filter fields to current compatible human labels", () => {
    expect(sqlAggregateFieldOptions(aggregateForm, "sum").map((field) =>
      field.label
    )).toEqual(["Amount", "Long value"]);
    expect(sqlAggregateFieldOptions(aggregateForm, "count-values").map((field) =>
      field.label
    )).toEqual(["Amount", "Category", "Long value", "Starts at"]);
    expect(sqlAggregateFieldOptions(aggregateForm, "minimum").map((field) =>
      field.label
    )).toEqual(["Amount", "Long value", "Starts at"]);
    expect(sqlFilterFieldOptions(aggregateForm).map((field) => [
      field.label,
      field.parameterType,
    ])).toEqual([
      ["Amount", "integer"],
      ["Category", "string"],
    ]);
  });

  it("rejects aggregate and filter fields unavailable in the current Form", () => {
    expect(() => buildSqlAggregateQuery(aggregateForm, "sum", "Category"))
      .toThrow("The aggregate field is unavailable");
    expect(() =>
      buildSqlAggregateQuery(aggregateForm, "count-rows", "", "LongValue")
    ).toThrow("The filter field is unavailable");
    expect(() =>
      buildSqlAggregateQuery(aggregateForm, "count-rows", "", "StartsAt")
    ).toThrow("The filter field is unavailable");
  });

  it("does not extract placeholders from SQL literals or comments", () => {
    expect(normalizeSqlVariables(
      "SELECT * FROM form_entry WHERE title = '$literal' AND title = $actual -- $comment\n/* {{block}} */",
    )).toEqual({
      sql:
        "SELECT * FROM form_entry WHERE title = '$literal' AND title = $actual -- $comment\n/* {{block}} */",
      variables: [{ type: "string", name: "actual", description: "" }],
    });
    expect(normalizeSqlVariables(
      "SELECT * FROM form_entry WHERE title = {{title}}",
    )).toEqual({
      sql: "SELECT * FROM form_entry WHERE title = $title",
      variables: [{ type: "string", name: "title", description: "" }],
    });
  });

  it("should expose no SQL schema without backend Form metadata", () => {
    const schema = buildSqlSchema([]);
    expect(schema.tables).toEqual({});
  });

  it("stores __proto__ as an ordinary schema table", () => {
    const schema = buildSqlSchema([
      {
        id: "00000000-0000-0000-0000-000000000002",
        name: "__proto__",
        version: 1,
        template: "",
        fields: {},
      },
    ]);
    expect(Object.keys(schema.tables ?? {})).toContain("__proto__");
    expect(Object.prototype.hasOwnProperty.call(schema.tables, "__proto__"))
      .toBe(true);
    expect((schema.tables as Record<string, string[]>)["__proto__"]).toEqual([
      "_ugoite_id",
      "_ugoite_created_at",
      "_ugoite_updated_at",
    ]);
  });

  it("should include form fields in schema", () => {
    const forms: Form[] = [
      {
        id: "00000000-0000-0000-0000-000000000001",
        name: "Meeting",
        sql_relation: "form_00000000000000000000000000000001",
        version: 1,
        template: "# Meeting\n\n## Date\n",
        fields: {
          Date: { type: "date", required: false, sql_column: "field_104" },
        },
      },
    ];
    const schema = buildSqlSchema(forms);
    const columns = schema.tables?.Meeting;
    expect(columns).toEqual([
      "_ugoite_id",
      "_ugoite_created_at",
      "_ugoite_updated_at",
      "field_104",
    ]);
    expect(schema.tables).not.toHaveProperty(
      "form_00000000000000000000000000000001",
    );
    expect(columns).not.toContain("id");
    expect(columns).not.toContain("title");
    expect(columns).not.toContain("updated_at");
  });

  it("should flag empty query", () => {
    const diagnostics = sqlLintDiagnostics("");
    expect(diagnostics.some((d) => d.message.includes("required"))).toBe(true);
  });

  it("should flag missing FROM clause", () => {
    const diagnostics = sqlLintDiagnostics("SELECT 1");
    expect(diagnostics.some((d) => d.message.includes("FROM"))).toBe(true);
  });

  it("should warn about multiple statements", () => {
    const diagnostics = sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001"; SELECT 1 FROM foo',
    );
    expect(diagnostics.some((d) => d.message.includes("single statement")))
      .toBe(true);
  });

  it("keeps LIMIT lint advisory for native and template parameters", () => {
    expect(
      sqlLintDiagnostics(
        'SELECT * FROM "form_entry" LIMIT abc',
      ).some((diagnostic) => diagnostic.message.includes("LIMIT")),
    ).toBe(true);
    expect(sqlLintDiagnostics(
      "SELECT * FROM form_entry LIMIT $page_size",
    )).toHaveLength(0);
    expect(sqlLintDiagnostics(
      "SELECT * FROM form_entry LIMIT {{page_size}}",
    )).toHaveLength(0);
  });

  it("pins #2318: SELECT then INSERT warns once without re-classifying later statements", () => {
    // Browser lint stays advisory and first-statement-only: the read-only
    // verdict comes from the first statement, and later statements are never
    // classified or enforced here. The server remains authoritative.
    const diagnostics = sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001"; INSERT INTO entries SELECT * FROM source',
    );
    expect(diagnostics.map((diagnostic) => diagnostic.message)).toEqual([
      "Only a single statement is supported",
    ]);
    expect(diagnostics[0]?.severity).toBe("warning");
  });

  it("ignores trailing semicolon trivia instead of a second statement (#2319)", () => {
    const base = 'SELECT * FROM "form_00000000000000000000000000000001"';
    for (
      const query of [
        `${base};`,
        `${base};   `,
        `${base}; -- trailing comment`,
        `${base}; /* trailing comment */`,
        `${base};   \n  -- trailing comment\n  /* block */  `,
      ]
    ) {
      const diagnostics = sqlLintDiagnostics(query);
      expect(
        diagnostics.some((diagnostic) =>
          diagnostic.message.includes("single statement")
        ),
        query,
      ).toBe(false);
      expect(diagnostics, query).toHaveLength(0);
    }
  });

  it("ignores leading comments when classifying the first statement (#2319)", () => {
    expect(sqlLintDiagnostics(
      '-- leading comment\n/* block lead */ SELECT * FROM "form_00000000000000000000000000000001"',
    )).toHaveLength(0);
  });

  it("ignores semicolons and keywords inside string literals (#2319)", () => {
    const diagnostics = sqlLintDiagnostics(
      "SELECT * FROM \"form_00000000000000000000000000000001\" WHERE title = '; DROP TABLE x; INSERT INTO y SELECT * FROM z'",
    );
    expect(
      diagnostics.some((diagnostic) =>
        diagnostic.message.includes("single statement")
      ),
    ).toBe(false);
    expect(diagnostics).toHaveLength(0);
  });

  it("masks quoted identifiers for the single-statement scan (#2319)", () => {
    // `;` and SQL keywords inside "quoted identifiers" must not create a
    // second-statement warning nor change the first-statement SELECT verdict.
    for (
      const query of [
        'SELECT "select; insert" FROM "form_00000000000000000000000000000001"',
        'SELECT * FROM "form_00000000000000000000000000000001" WHERE "weird;col" = \'x\'',
      ]
    ) {
      const diagnostics = sqlLintDiagnostics(query);
      expect(
        diagnostics.some((diagnostic) =>
          diagnostic.message.includes("single statement")
        ),
        query,
      ).toBe(false);
      expect(
        diagnostics.some((diagnostic) =>
          diagnostic.message.includes("must start with SELECT")
        ),
        query,
      ).toBe(false);
    }
  });
});

const aggregateForm: Form = {
  id: "form-id",
  name: "Orders",
  sql_relation: "form_internal_relation",
  version: 1,
  template: "",
  fields: {
    Amount: {
      id: 1,
      type: "integer",
      required: false,
      label: "Amount",
      sql_column: "field_amount",
      query_capability: {
        field: { kind: "property", field_id: 1 },
        name: "Amount",
        field_type: "integer",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals", "gt"],
      },
    },
    Category: {
      id: 2,
      type: "string",
      required: false,
      label: "Category",
      sql_column: "field_category",
      query_capability: {
        field: { kind: "property", field_id: 2 },
        name: "Category",
        field_type: "string",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals", "contains"],
      },
    },
    LongValue: {
      id: 3,
      type: "long",
      required: false,
      label: "Long value",
      sql_column: "field_long",
      query_capability: {
        field: { kind: "property", field_id: 3 },
        name: "Long value",
        field_type: "long",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals"],
      },
    },
    Unavailable: {
      id: 4,
      type: "float",
      required: false,
      label: "Unavailable",
      sql_column: "field_unavailable",
      query_capability: {
        field: { kind: "property", field_id: 4 },
        name: "Unavailable",
        field_type: "float",
        filterable: false,
        sortable: false,
        projectable: false,
        supported_operators: [],
      },
    },
    MissingColumn: {
      id: 5,
      type: "integer",
      required: false,
      label: "Missing column",
    },
    StartsAt: {
      id: 7,
      type: "timestamp_tz",
      required: false,
      label: "Starts at",
      sql_column: "field_starts_at",
      query_capability: {
        field: { kind: "property", field_id: 7 },
        name: "Starts at",
        field_type: "timestamp_tz",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals"],
      },
    },
    "00000000-0000-4000-8000-000000000006": {
      id: 6,
      type: "string",
      required: false,
      sql_column: "field_opaque",
      query_capability: {
        field: { kind: "property", field_id: 6 },
        name: "Field 6",
        field_type: "string",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals"],
      },
    },
  },
};
