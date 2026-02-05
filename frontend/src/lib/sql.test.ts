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
});
