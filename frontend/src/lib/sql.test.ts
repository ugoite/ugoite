// REQ-FE-036: SQL Query Editor
import { buildSqlSchema, sqlLintDiagnostics } from "./sql";
import type { Class } from "./types";

describe("sql helpers", () => {
	it("should flag missing select clause", () => {
		const diagnostics = sqlLintDiagnostics("FROM notes");
		expect(diagnostics.some((diag) => diag.message.includes("SELECT"))).toBe(true);
	});

	it("should accept valid select queries", () => {
		const diagnostics = sqlLintDiagnostics("SELECT * FROM notes LIMIT 10");
		expect(diagnostics).toHaveLength(0);
	});

	it("should include notes table in schema", () => {
		const schema = buildSqlSchema([]);
		expect(schema.tables?.notes).toContain("title");
	});

	it("should include class fields in schema", () => {
		const classes: Class[] = [
			{
				name: "Meeting",
				version: 1,
				template: "# Meeting\n\n## Date\n",
				fields: { Date: { type: "date", required: false } },
			},
		];
		const schema = buildSqlSchema(classes);
		expect(schema.tables?.Meeting).toContain("Date");
		expect(schema.tables?.notes).toContain("Date");
	});
});
