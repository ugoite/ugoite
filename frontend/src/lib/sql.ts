import type { Diagnostic } from "@codemirror/lint";
import type { SQLConfig } from "@codemirror/lang-sql";
import sqlRules from "../../../shared/sql/ugoite-sql-rules.json";
import type { Form } from "./types";

export type SqlSchema = NonNullable<SQLConfig["schema"]>;

/* v8 ignore start */
const BASE_COLUMNS = [...(sqlRules.base_columns ?? [])];
const BASE_TABLES = [...(sqlRules.base_tables ?? ["entries"])];
/* v8 ignore stop */

export function buildSqlSchema(forms: Form[]): SqlSchema {
	const formFieldSet = new Set<string>();
	for (const item of forms) {
		/* v8 ignore start */
		for (const field of Object.keys(item.fields ?? {})) {
			/* v8 ignore stop */
			formFieldSet.add(field);
		}
	}

	const unionFields = [...BASE_COLUMNS, ...formFieldSet];
	const tables: Record<string, string[]> = {};
	for (const table of BASE_TABLES) {
		tables[table] = unionFields;
	}

	for (const item of forms) {
		/* v8 ignore start */
		tables[item.name] = [...BASE_COLUMNS, ...Object.keys(item.fields ?? {})];
		/* v8 ignore stop */
	}

	return { tables };
}

export function sqlLintDiagnostics(query: string): Diagnostic[] {
	const diagnostics: Diagnostic[] = [];
	/* v8 ignore start */
	const lintRules = sqlRules.lint ?? {};
	/* v8 ignore stop */
	const leadingWhitespace = query.length - query.trimStart().length;
	const trimmed = query.trim();
	if (!trimmed) {
		diagnostics.push({
			from: 0,
			to: Math.min(query.length, 1),
			severity: "error",
			message: "Query is required",
		});
		return diagnostics;
	}

	const selectMatch = /\bselect\b/i.exec(trimmed);
	if (lintRules.require_select !== false && !selectMatch) {
		diagnostics.push({
			from: leadingWhitespace,
			to: leadingWhitespace + Math.min(trimmed.length, 6),
			severity: "error",
			message: "Query must start with SELECT",
		});
	}

	const fromMatch = /\bfrom\b/i.exec(trimmed);
	if (lintRules.require_from !== false && !fromMatch) {
		diagnostics.push({
			from: Math.max(0, query.length - 1),
			to: query.length,
			severity: "error",
			message: "Query must include FROM",
		});
	}

	const semicolonIndex = query.indexOf(";");
	/* v8 ignore start */
	if (lintRules.single_statement_only !== false) {
		/* v8 ignore stop */
		if (semicolonIndex !== -1 && semicolonIndex < query.length - 1) {
			diagnostics.push({
				from: semicolonIndex,
				to: semicolonIndex + 1,
				severity: "warning",
				message: "Only a single statement is supported",
			});
		}
		/* v8 ignore start */
	}
	/* v8 ignore stop */

	const limitMatch = /\blimit\b\s+([^\s;]+)/i.exec(query);
	/* v8 ignore start */
	if (lintRules.limit_requires_number !== false) {
		/* v8 ignore stop */
		if (limitMatch && Number.isNaN(Number(limitMatch[1]))) {
			const from = limitMatch.index + limitMatch[0].indexOf(limitMatch[1]);
			diagnostics.push({
				from,
				to: from + limitMatch[1].length,
				severity: "error",
				message: "LIMIT value must be a number",
			});
		}
		/* v8 ignore start */
	}
	/* v8 ignore stop */

	return diagnostics;
}
