// REQ-FE-036: SQL Query Editor
import {
  applySqlVariableDefaultDrafts,
  buildSqlSchema,
  normalizeSqlDocument,
  normalizeSqlVariables,
  parseSqlVariableDefault,
  sqlLintDiagnostics,
  sqlSchemaFormLabelsFor,
} from "./sql";
import { normalizeSqlTemplate } from "./ugoite-client";
import type { Form } from "./types";

describe("sql helpers", () => {
  it("should flag incomplete SQL through the portable Rust linter", async () => {
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

  it("reports the SELECT-only policy from the portable Rust linter", async () => {
    expect(
      (await sqlLintDiagnostics("DROP TABLE entries")).map((diag) =>
        diag.message
      ),
    ).toEqual(["SQL must be a SELECT query"]);
  });

  it("classifies read-only policy from the top-level statement form", async () => {
    expect(
      (await sqlLintDiagnostics("INSERT INTO entries SELECT * FROM source"))
        .map((
          diagnostic,
        ) => diagnostic.message),
    ).toEqual(["SQL must be a SELECT query"]);

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
          diagnostic.message === "SQL must be a SELECT query"
        ),
        query,
      ).toBe(true);
    }
  });

  it("accepts a scalar SELECT with a nested relation-free query", async () => {
    expect(await sqlLintDiagnostics("SELECT (SELECT 1)")).toEqual([]);
  });

  it("checks projection wildcards only for Composition and maps UTF-8 ranges", async () => {
    const sql = "SELECT '🧡', 店.* FROM records";
    expect(await sqlLintDiagnostics(sql, "standard")).toEqual([]);
    const diagnostics = await sqlLintDiagnostics(sql, "composition");
    expect(diagnostics).toMatchObject([{
      from: sql.indexOf("店.*"),
      to: sql.indexOf("店.*") + "店.*".length,
      severity: "error",
      message: "Composition SQL must list projected columns explicitly",
    }]);
  });

  it("accepts a SELECT whose top-level form is introduced by a CTE", async () => {
    expect(
      await sqlLintDiagnostics("WITH rows AS (SELECT 1) SELECT * FROM rows"),
    )
      .toEqual(
        [],
      );
  });

  it("rejects non-SELECT statements even when they include a SELECT", async () => {
    expect(
      await sqlLintDiagnostics(
        "WITH source AS (SELECT * FROM entries) INSERT INTO entries SELECT * FROM source",
      ),
    ).toMatchObject([
      { message: "SQL query bodies must use SELECT", severity: "error" },
    ]);
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

  it("does not treat a dollar-quoted template lookalike as a parameter", async () => {
    expect(
      await normalizeSqlVariables("SELECT $$ {{label}} $$ AS value"),
    ).toEqual({
      sql: "SELECT $$ {{label}} $$ AS value",
      variables: [],
    });
  });

  it("keeps SQL token boundaries authoritative with Unicode offsets and duplicate variables", async () => {
    const result = await normalizeSqlDocument(
      `SELECT 'café 🧪', {{label}}, {{label}}, '''{{triple}}''', "{{identifier}}"`,
    );
    expect(result.sql).toBe(
      `SELECT 'café 🧪', $label, $label, '''{{triple}}''', "{{identifier}}"`,
    );
    expect(result.variables.map(({ name }) => name)).toEqual(["label"]);
    expect(result.replacements).toHaveLength(2);
    expect(result.replacements[0].start).toBe(`SELECT 'café 🧪', `.length);
    expect(result.replacements[1].start).toBe(
      result.source.indexOf("{{label}}", result.replacements[0].end),
    );
    expect(result.document.source).toBe(result.sql);
  });

  it("keeps incomplete drafts editable and preserves malformed-marker diagnostics", async () => {
    const incomplete = await normalizeSqlDocument("SELECT {{minimum}} +");
    expect(incomplete.sql).toBe("SELECT $minimum +");
    expect(incomplete.variables.map(({ name }) => name)).toEqual(["minimum"]);
    expect(incomplete.document.status).toBe("invalid_sql");

    const malformed = await normalizeSqlDocument(
      "SELECT $one, {{bad-name}}, {{two}}, {{also-bad}}, $tail",
    );
    expect(malformed.sql).toBe(
      "SELECT $one, {{bad-name}}, $two, {{also-bad}}, $tail",
    );
    expect(malformed.variables.map(({ name }) => name)).toEqual([
      "one",
      "two",
      "tail",
    ]);
    expect(malformed.document.status).toBe("invalid_sql");
    expect(malformed.document.diagnostics.length).toBeGreaterThan(0);
  });

  it("keeps the documented SQL Document size limit explicit in the browser protocol", async () => {
    const sql = `SELECT {{name}}${" ".repeat(256 * 1024)}`;
    const normalized = await normalizeSqlDocument(sql);
    expect(normalized.sql).toBe(sql);
    expect(normalized.variables).toEqual([]);
    expect(normalized.document.status).toBe("too_large");
  });

  it("keeps dense template edit metadata below the actual WASM response limit", async () => {
    const count = 10_000;
    const normalized = await normalizeSqlTemplate("{{a}},".repeat(count));
    expect(normalized.sql).toBe("$a,".repeat(count));
    expect(normalized.parameters).toEqual(["a"]);
    expect(normalized.replacements).toHaveLength(count);
  });

  it("keeps maximum-size dense template edits below the actual WASM response limit", async () => {
    const count = Math.floor((256 * 1024) / "{{a}}".length);
    const sql = "{{a}}".repeat(count);
    const normalized = await normalizeSqlTemplate(sql);
    expect(new TextEncoder().encode(sql).length).toBeLessThanOrEqual(256 * 1024);
    expect(normalized.replacements).toHaveLength(count);
    expect(normalized.sql).toBe("$a".repeat(count));
  });

  it("keeps maximum-size unique parameter metadata below the actual WASM response limit", async () => {
    const alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_";
    const count = Math.floor((256 * 1024) / 7);
    const nameAt = (index: number): string => {
      const name = ["a", "a", "a"];
      for (let position = 2; position >= 0; position -= 1) {
        name[position] = alphabet[index % alphabet.length];
        index = Math.floor(index / alphabet.length);
      }
      return name.join("");
    };
    const names = Array.from({ length: count }, (_, index) => nameAt(index));
    const sql = names.map((name) => `{{${name}}}`).join("");
    const normalized = await normalizeSqlTemplate(sql);
    expect(new TextEncoder().encode(sql).length).toBeLessThanOrEqual(256 * 1024);
    expect(normalized.parameters).toEqual(names);
    expect(normalized.replacements).toHaveLength(count);
    expect(normalized.sql).toBe(names.map((name) => `$${name}`).join(""));
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

  it("should flag empty query", async () => {
    const diagnostics = await sqlLintDiagnostics("");
    expect(diagnostics.some((d) => d.message.includes("must not be empty")))
      .toBe(true);
  });

  it("should reject multiple statements", async () => {
    const diagnostics = await sqlLintDiagnostics(
      'SELECT * FROM "form_00000000000000000000000000000001"; SELECT 1 FROM foo',
    );
    expect(diagnostics.some((d) => d.message.includes("one SELECT statement")))
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
      "SQL must contain one SELECT statement",
    ]);
    expect(diagnostics[0]?.severity).toBe("error");
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
          diagnostic.message.includes("one SELECT statement")
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
        diagnostic.message.includes("one SELECT statement")
      ),
    ).toBe(false);
    expect(diagnostics).toHaveLength(0);
  });

  it("keeps quoted identifiers opaque to Rust statement parsing", async () => {
    // `;` and SQL keywords inside "quoted identifiers" must not create a
    // second-statement error nor change the first-statement SELECT verdict.
    for (
      const query of [
        'SELECT "select; insert" FROM "form_00000000000000000000000000000001"',
        'SELECT * FROM "form_00000000000000000000000000000001" WHERE "weird;col" = \'x\'',
      ]
    ) {
      const diagnostics = await sqlLintDiagnostics(query);
      expect(
        diagnostics.some((diagnostic) =>
          diagnostic.message.includes("one SELECT statement")
        ),
        query,
      ).toBe(false);
      expect(
        diagnostics.some((diagnostic) =>
          diagnostic.message.includes("must be a SELECT query")
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
