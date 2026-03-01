// REQ-FE-036: SQL Query Editor
import { buildSqlSchema, sqlLintDiagnostics } from "./sql";
import type { Form } from "./types";

describe("sql helpers", () => {
	it("should flag missing select clause", () => {
		const diagnostics = sqlLintDiagnostics("FROM entries");
		expect(diagnostics.some((diag) => diag.message.includes("SELECT"))).toBe(true);
	});

	it("should accept valid select queries", () => {
		const diagnostics = sqlLintDiagnostics("SELECT * FROM entries LIMIT 10");
		expect(diagnostics).toHaveLength(0);
	});

	it("should include entries table in schema", () => {
		const schema = buildSqlSchema([]);
		expect(schema.tables?.entries).toContain("title");
	});

	it("should include form fields in schema", () => {
		const forms: Form[] = [
			{
				name: "Meeting",
				version: 1,
				template: "# Meeting\n\n## Date\n",
				fields: { Date: { type: "date", required: false } },
			},
		];
		const schema = buildSqlSchema(forms);
		expect(schema.tables?.Meeting).toContain("Date");
		expect(schema.tables?.entries).toContain("Date");
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
		const diagnostics = sqlLintDiagnostics("SELECT * FROM entries; SELECT 1 FROM foo");
		expect(diagnostics.some((d) => d.message.includes("single statement"))).toBe(true);
	});

	it("should flag invalid LIMIT value", () => {
		const diagnostics = sqlLintDiagnostics("SELECT * FROM entries LIMIT abc");
		expect(diagnostics.some((d) => d.message.includes("LIMIT"))).toBe(true);
	});
});
