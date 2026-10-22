// REQ-FE-036: SQL Query Editor
import {
  applySqlVariableDefaultDrafts,
  buildSqlSchema,
  buildSqlStructureContexts,
  mergeSqlStructureVariables,
  normalizeSqlVariables,
  parseSqlVariableDefault,
  sqlLintDiagnostics,
  sqlSchemaFormLabelsFor,
} from "./sql";
import type { Form } from "./types";

describe("sql helpers", () => {
  it("should flag incomplete SQL from the Rust SQL Document", async () => {
    const diagnostics = await sqlLintDiagnostics("SELECT (");
    expect(diagnostics.length).toBeGreaterThan(0);
  });

  it("should accept valid select queries", async () => {
    const diagnostics = await sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001" LIMIT 10',
    );
    expect(diagnostics).toHaveLength(0);
  });

  it("accepts a read-only scalar SELECT without a FROM clause", async () => {
    expect(await sqlLintDiagnostics("SELECT 1")).toEqual([]);
  });

  it("accepts read-only SQL with numeric aggregate expressions", async () => {
    const diagnostics = await sqlLintDiagnostics(
      'SELECT COUNT(*) AS total, SUM(amount) AS sum, AVG(amount) AS average, MIN(amount) AS minimum, MAX(amount) AS maximum FROM "Expenses" WHERE amount IS NOT NULL',
    );

    expect(diagnostics).toEqual([]);
  });

  it("leaves planner-level LIMIT validation to the Rust executor", async () => {
    expect(
      await sqlLintDiagnostics(
        'SELECT * FROM "form_00000000000000000000000000000001" LIMIT invalid',
      ),
    ).toEqual([]);
  });

  it("reports the read-only statement policy from Rust SQL Document", async () => {
    expect(
      (await sqlLintDiagnostics("DROP TABLE entries")).map((diag) =>
        diag.message
      ),
    ).toEqual(["Read-only query must start with SELECT"]);
  });

  it("classifies read-only policy from the top-level statement form", async () => {
    expect(
      (await sqlLintDiagnostics("INSERT INTO entries SELECT * FROM source"))
        .map((
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
        (await sqlLintDiagnostics(query)).some((diagnostic) =>
          diagnostic.message === "Read-only query must start with SELECT"
        ),
        query,
      ).toBe(true);
    }
  });

  it("accepts a scalar SELECT with a nested relation-free query", async () => {
    expect(await sqlLintDiagnostics("SELECT (SELECT 1)")).toEqual([]);
  });

  it("accepts a SELECT whose top-level form is introduced by a CTE", async () => {
    expect(
      await sqlLintDiagnostics("WITH rows AS (SELECT 1) SELECT * FROM rows"),
    )
      .toEqual(
        [],
      );
  });

  it("uses Rust SQL Document as the source of statement diagnostics", async () => {
    expect(
      await sqlLintDiagnostics(
        "WITH source AS (SELECT * FROM entries) INSERT INTO entries SELECT * FROM source",
      ),
    ).toEqual([]);
  });

  it("builds SQL structure capabilities from trusted Form columns and typed variables", () => {
    const [context] = buildSqlStructureContexts([{
      id: "form-1",
      name: "Tasks",
      sql_relation: "form_tasks",
      version: 1,
      template: "task",
      fields: {
        title: { type: "string", required: false, sql_column: "field_title" },
        amount: {
          type: "float",
          required: false,
          sql_column: "field_amount",
          query_capability: {
            field: { kind: "property", field_id: 1 },
            name: "Amount",
            field_type: "float",
            filterable: true,
            sortable: true,
            projectable: true,
            supported_operators: ["equals"],
          },
        },
        hidden: {
          type: "string",
          required: false,
          sql_column: "field_hidden",
          query_capability: {
            field: { kind: "property", field_id: 2 },
            name: "Hidden",
            field_type: "string",
            filterable: false,
            sortable: false,
            projectable: false,
            supported_operators: [],
          },
        },
      },
    }], [
      { name: "month", type: "date" },
      { name: "complex", type: "json" },
    ], "composition:source-1");

    expect(context).toMatchObject({
      identity: expect.stringContaining("composition:source-1"),
      label: "Tasks",
      parser: {
        relation: "form_tasks",
        columns: [
          { name: "_ugoite_id", type: "string" },
          { name: "_ugoite_created_at", type: "timestamp" },
          { name: "_ugoite_updated_at", type: "timestamp" },
          { name: "field_title", type: "string" },
          { name: "field_amount", type: "float" },
          { name: "field_hidden", type: "string" },
        ],
        variables: [{ name: "month", type: "date" }],
        stable_key: ["_ugoite_id"],
      },
      columnCapabilities: {
        field_amount: {
          projectable: true,
          filterable: true,
          sortable: true,
          supportedOperators: [
            "eq",
            "in",
            "not_in",
            "between",
            "not_between",
            "is_null",
            "is_not_null",
          ],
          aggregateFunctions: expect.any(Array),
          groupable: true,
          distinctable: true,
        },
        field_hidden: {
          projectable: false,
          filterable: false,
          sortable: false,
          supportedOperators: [],
          aggregateFunctions: [],
          groupable: false,
          distinctable: false,
        },
      },
    });
  });

  it("uses Rust SQL tokens to exclude placeholders in literals or comments", async () => {
    expect(
      await normalizeSqlVariables(
        "SELECT * FROM form_entry WHERE title = '$literal' AND title = $actual -- $comment\n/* {{block}} */",
      ),
    ).toEqual({
      sql:
        "SELECT * FROM form_entry WHERE title = '$literal' AND title = $actual -- $comment\n/* {{block}} */",
      variables: [{ type: "string", name: "actual", description: "" }],
    });
    expect(
      await normalizeSqlVariables(
        "SELECT * FROM form_entry WHERE title = {{ title }}",
      ),
    ).toEqual({
      sql: "SELECT * FROM form_entry WHERE title = $title",
      variables: [{ type: "string", name: "title", description: "" }],
    });
    expect(await normalizeSqlVariables("SELECT {{label}}"))
      .toEqual({
        sql: "SELECT $label",
        variables: [{ type: "string", name: "label", description: "" }],
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
        sql_relation: "__proto__",
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
    const columns = schema.tables?.form_00000000000000000000000000000001;
    expect(columns).toEqual([
      "_ugoite_id",
      "_ugoite_created_at",
      "_ugoite_updated_at",
      "field_104",
    ]);
    expect(schema.tables).not.toHaveProperty("Meeting");
    expect(columns).not.toContain("id");
    expect(columns).not.toContain("title");
    expect(columns).not.toContain("updated_at");
  });

  it("uses the server SQL relation for Form structure contexts", () => {
    const form: Form = {
      id: "stable-form-id",
      name: "Renamed Form",
      sql_relation: "form_00000000000000000000000000000001",
      version: 1,
      template: "",
      fields: {},
    };
    const [formContext] = buildSqlStructureContexts([form]);
    const schema = buildSqlSchema([form]);

    expect(formContext).toMatchObject({
      label: "Renamed Form",
      parser: { relation: "form_00000000000000000000000000000001" },
    });
    expect(schema.tables).toHaveProperty(
      "form_00000000000000000000000000000001",
    );
    expect(schema.tables).not.toHaveProperty("Renamed Form");
  });

  it("should flag empty query", async () => {
    const diagnostics = await sqlLintDiagnostics("");
    expect(diagnostics.some((d) => d.message.includes("required"))).toBe(true);
  });

  it("should warn about multiple statements", async () => {
    const diagnostics = await sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001"; SELECT 1 FROM foo',
    );
    expect(diagnostics.some((d) => d.message.includes("single statement")))
      .toBe(true);
  });

  it("uses Rust SQL parsing for native and template parameters", async () => {
    expect(
      await sqlLintDiagnostics(
        'SELECT * FROM "form_entry" LIMIT abc',
      ),
    ).toEqual([]);
    expect(
      await sqlLintDiagnostics(
        "SELECT * FROM form_entry LIMIT $page_size",
      ),
    ).toHaveLength(0);
    expect(
      await sqlLintDiagnostics(
        "SELECT * FROM form_entry LIMIT {{page_size}}",
      ),
    ).toHaveLength(0);
  });

  it("uses Rust diagnostics for multiple statements", async () => {
    const diagnostics = await sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001"; INSERT INTO entries SELECT * FROM source',
    );
    expect(diagnostics.map((diagnostic) => diagnostic.message)).toEqual([
      "Only a single statement is supported",
    ]);
    expect(diagnostics[0]?.severity).toBe("warning");
  });

  it("ignores trailing semicolon trivia instead of a second statement", async () => {
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
      const diagnostics = await sqlLintDiagnostics(query);
      expect(
        diagnostics.some((diagnostic) =>
          diagnostic.message.includes("single statement")
        ),
        query,
      ).toBe(false);
      expect(diagnostics, query).toHaveLength(0);
    }
  });

  it("ignores leading comments when classifying the first statement", async () => {
    expect(
      await sqlLintDiagnostics(
        '-- leading comment\n/* block lead */ SELECT * FROM "form_00000000000000000000000000000001"',
      ),
    ).toHaveLength(0);
  });

  it("ignores semicolons and keywords inside string literals", async () => {
    const diagnostics = await sqlLintDiagnostics(
      "SELECT * FROM \"form_00000000000000000000000000000001\" WHERE title = '; DROP TABLE x; INSERT INTO y SELECT * FROM z'",
    );
    expect(
      diagnostics.some((diagnostic) =>
        diagnostic.message.includes("single statement")
      ),
    ).toBe(false);
    expect(diagnostics).toHaveLength(0);
  });

  it("keeps quoted identifiers opaque to Rust statement parsing", async () => {
    // `;` and SQL keywords inside "quoted identifiers" must not create a
    // second-statement warning nor change the first-statement SELECT verdict.
    for (
      const query of [
        'SELECT "select; insert" FROM "form_00000000000000000000000000000001"',
        'SELECT * FROM "form_00000000000000000000000000000001" WHERE "weird;col" = \'x\'',
      ]
    ) {
      const diagnostics = await sqlLintDiagnostics(query);
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

  it("preserves saved variable types and defaults while SQL placeholders remain", async () => {
    const existing = [
      { type: "integer", name: "count", description: "Count", default: 7 },
      { type: "string", name: "title", description: "Title", default: "draft" },
    ];
    expect(
      (await normalizeSqlVariables("SELECT $title, $count", existing))
        .variables,
    ).toEqual([existing[1], existing[0]]);
    expect((await normalizeSqlVariables("SELECT $title", existing)).variables)
      .toEqual([
        existing[1],
      ]);
    expect(
      (await normalizeSqlVariables(
        "SELECT * FROM form_entry WHERE title = {{ title }}",
        existing,
      )).variables,
    ).toEqual([existing[1]]);
  });

  it("replaces a stale saved declaration when a structure action reuses its name", () => {
    expect(mergeSqlStructureVariables([
      {
        type: "string",
        name: "filter_amount",
        description: "Old",
        default: "x",
      },
      {
        type: "integer",
        name: "threshold",
        description: "Threshold",
        default: 4,
      },
    ], [{ name: "filter_amount", type: "integer" }])).toEqual([
      { type: "integer", name: "filter_amount", description: "" },
      {
        type: "integer",
        name: "threshold",
        description: "Threshold",
        default: 4,
      },
    ]);
  });

  it("keeps default presence separate from empty and typed variable values", () => {
    const variables = [
      { type: "string", name: "empty", description: "" },
      { type: "boolean", name: "enabled", description: "" },
      { type: "integer", name: "count", description: "" },
      { type: "date", name: "day", description: "" },
      { type: "timestamp", name: "at", description: "" },
    ];
    const result = applySqlVariableDefaultDrafts(variables, {
      empty: { enabled: true, raw: "" },
      enabled: { enabled: true, raw: "false" },
      count: { enabled: true, raw: "9007199254740992" },
      day: { enabled: true, raw: "2026-02-29" },
      at: { enabled: true, raw: "2026-10-15T09:00:00Z" },
    });

    expect(result).toEqual({
      variables: [],
      invalid: { name: "count", type: "integer" },
    });
    expect(parseSqlVariableDefault("string", "")).toEqual({
      valid: true,
      value: "",
    });
    expect(parseSqlVariableDefault("integer", "-7")).toEqual({
      valid: true,
      value: -7,
    });
    expect(parseSqlVariableDefault("float", "1e-324")).toEqual({
      valid: false,
    });
    expect(parseSqlVariableDefault("float", "0e-324")).toEqual({
      valid: true,
      value: 0,
    });
    expect(parseSqlVariableDefault("float", "-0.000e-324")).toEqual({
      valid: true,
      value: -0,
    });
    expect(parseSqlVariableDefault("date", "2026-02-28")).toEqual({
      valid: true,
      value: "2026-02-28",
    });
    expect(parseSqlVariableDefault("timestamp", "2026-10-15T09:00:00-07:00"))
      .toEqual({ valid: true, value: "2026-10-15T09:00:00-07:00" });
    expect(parseSqlVariableDefault("timestamp", "２０２６-10-15T09:00:00Z"))
      .toEqual({ valid: false });
    expect(parseSqlVariableDefault("timestamp", "2026-10-15T09:00:00-00:00"))
      .toEqual({ valid: false });
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

describe("SQL structure adapter context", () => {
  it("provides a relation-free context when no Form is selected", () => {
    const [context] = buildSqlStructureContexts([], [], "new-sql");

    expect(context.parser).toEqual({
      relation: "",
      columns: [],
      variables: [],
      stable_key: [],
    });
    expect(context.columnCapabilities).toEqual({});
  });

  it("uses the server-derived Form relation and intersects typed column capabilities", () => {
    const formWithUntrustedType: Form = {
      ...aggregateForm,
      fields: {
        ...aggregateForm.fields,
        UntrustedType: {
          id: 8,
          type: "integer",
          required: false,
          label: "Untrusted type",
          sql_column: "field_untrusted",
        },
      },
    };
    const [context] = buildSqlStructureContexts(
      [formWithUntrustedType],
      [
        { name: "amount", type: "integer" },
        { name: "unsafe_long", type: "long" },
        { name: "at", type: "timestamp" },
      ],
      "composition:inline-1",
    );

    expect(context.parser.relation).toBe(aggregateForm.sql_relation);
    expect(context.parser.relation).not.toBe("Orders");
    expect(context.parser.stable_key).toEqual(["_ugoite_id"]);
    expect(context.columnCapabilities.field_amount.supportedOperators).toEqual([
      "eq",
      "gt",
      "in",
      "not_in",
      "between",
      "not_between",
      "is_null",
      "is_not_null",
    ]);
    expect(context.columnCapabilities.field_unavailable.supportedOperators)
      .toEqual([]);
    expect(context.parser.columns).toContainEqual({
      name: "field_amount",
      type: "integer",
    });
    expect(context.parser.columns).toContainEqual({
      name: "field_long",
      type: "other",
    });
    expect(context.parser.variables).toEqual([
      { name: "amount", type: "integer" },
      { name: "at", type: "timestamp" },
    ]);
    expect(context.columnCapabilities.field_amount).toEqual({
      projectable: true,
      filterable: true,
      sortable: true,
      supportedOperators: [
        "eq",
        "gt",
        "in",
        "not_in",
        "between",
        "not_between",
        "is_null",
        "is_not_null",
      ],
      aggregateFunctions: ["count", "sum", "avg", "mean", "min", "max"],
      groupable: true,
      distinctable: true,
    });
    expect(context.columnCapabilities.field_category.supportedOperators)
      .toEqual([
        "eq",
        "in",
        "not_in",
        "between",
        "not_between",
        "like",
        "not_like",
        "ilike",
        "not_ilike",
        "is_null",
        "is_not_null",
      ]);
    expect(context.columnCapabilities.field_category.aggregateFunctions)
      .toEqual(["count", "min", "max"]);
    expect(context.columnCapabilities.field_category.groupable).toBe(true);
    expect(context.columnCapabilities.field_category.distinctable).toBe(true);
    expect(context.columnCapabilities.field_long.aggregateFunctions).toEqual(
      ["count"],
    );
    expect(context.columnCapabilities.field_long.groupable).toBe(false);
    expect(context.columnCapabilities.field_long.distinctable).toBe(false);
    expect(context.columnCapabilities.field_unavailable).toEqual({
      projectable: false,
      filterable: false,
      sortable: false,
      supportedOperators: [],
      aggregateFunctions: [],
      groupable: false,
      distinctable: false,
    });
    expect(context.parser.columns).toContainEqual({
      name: "field_untrusted",
      type: "integer",
    });
    expect(context.columnCapabilities.field_untrusted).toEqual({
      projectable: false,
      filterable: false,
      sortable: false,
      supportedOperators: [],
      aggregateFunctions: [],
      groupable: false,
      distinctable: false,
    });
    expect(context.identity).toContain("composition:inline-1");
    expect(context.identity).toContain('"formVersion":1');
  });

  it("keeps reserved Forms as analysis contexts but excludes them from SQL relation candidates", () => {
    const reservedForms: Form[] = [
      "SQL",
      "User",
      "UserGroup",
      "_ugoite_compositions",
    ].map((name, index) => ({
      id: `reserved-${index}`,
      name,
      sql_relation: `form_reserved_${index}`,
      version: 1,
      template: "",
      fields: {},
    }));
    const eligibleForm: Form = {
      ...aggregateForm,
      name: "Orders",
      sql_relation: "form_orders",
    };

    const contexts = buildSqlStructureContexts(
      [...reservedForms, eligibleForm],
      [],
      "new-sql",
    );

    expect(contexts.map((context) => context.label)).toEqual([
      "SQL",
      "User",
      "UserGroup",
      "_ugoite_compositions",
      "Orders",
      undefined,
    ]);
    expect(contexts.map((context) => context.parser.relation)).toEqual([
      "form_reserved_0",
      "form_reserved_1",
      "form_reserved_2",
      "form_reserved_3",
      "form_orders",
      "",
    ]);
    expect(contexts.map((context) => context.relationSelectable)).toEqual([
      false,
      false,
      false,
      false,
      true,
      false,
    ]);
  });

  it("changes its identity when the Form version changes", () => {
    const [current] = buildSqlStructureContexts([aggregateForm], [], "inline");
    const [updated] = buildSqlStructureContexts(
      [{
        ...aggregateForm,
        version: aggregateForm.version + 1,
      }],
      [],
      "inline",
    );

    expect(updated.identity).not.toBe(current.identity);
  });

  it("keeps the Form-backed SQL relation stable when its display name changes", () => {
    const renamedForm: Form = {
      ...aggregateForm,
      name: "Archived orders",
      version: aggregateForm.version + 1,
    };
    const [before] = buildSqlStructureContexts([aggregateForm], [], "new-sql");
    const [after] = buildSqlStructureContexts([renamedForm], [], "new-sql");
    const [sameNameNewForm] = buildSqlStructureContexts(
      [{
        ...aggregateForm,
        id: "another-form-id",
        sql_relation: "form_another_relation",
      }],
      [],
      "new-sql",
    );
    const beforeSchema = buildSqlSchema([aggregateForm]);
    const afterSchema = buildSqlSchema([renamedForm]);
    expect(after.parser.relation).toBe(before.parser.relation);
    expect(after.label).toBe("Archived orders");
    expect(after.identity).not.toBe(before.identity);
    expect(sameNameNewForm.label).toBe("Orders");
    expect(sameNameNewForm.parser.relation).not.toBe(before.parser.relation);
    expect(sqlSchemaFormLabelsFor(beforeSchema)[aggregateForm.sql_relation!])
      .toBe("Orders");
    expect(sqlSchemaFormLabelsFor(afterSchema)[renamedForm.sql_relation!])
      .toBe("Archived orders");
  });
});
