// REQ-FE-036: SQL Query Editor
import {
  buildSqlSchema,
  buildSqlStarterQuery,
  normalizeSqlVariables,
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

  it("builds a starter query with the SQL session total order", () => {
    expect(buildSqlStarterQuery("form_entry")).toBe(
      'SELECT * FROM "form_entry" ORDER BY _ugoite_updated_at DESC, _ugoite_id LIMIT 50',
    );
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
      "_ugoite_title",
      "_ugoite_created_at",
      "_ugoite_updated_at",
      "field_104",
    ]);
    expect(schema.tables).not.toHaveProperty("entries");
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
});
