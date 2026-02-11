import type { Diagnostic } from "@codemirror/lint";
import type { SQLConfig } from "@codemirror/lang-sql";
import sqlRules from "../../../shared/sql/ugoite-sql-rules.json";
import type { Form } from "./types";

export type SqlSchema = NonNullable<SQLConfig["schema"]>;

const BASE_COLUMNS = [...(sqlRules.base_columns ?? [])];
const BASE_TABLES = [...(sqlRules.base_tables ?? ["entries"])];

export function buildSqlSchema(forms: Form[]): SqlSchema {
	const formFieldSet = new Set<string>();
	for (const item of forms) {
		for (const field of Object.keys(item.fields ?? {})) {
			formFieldSet.add(field);
		}
	}

	const unionFields = [...BASE_COLUMNS, ...formFieldSet];
	const tables: Record<string, string[]> = {};
	for (const table of BASE_TABLES) {
		tables[table] = unionFields;
	}

	for (const item of forms) {
		tables[item.name] = [...BASE_COLUMNS, ...Object.keys(item.fields ?? {})];
	}

	return { tables };
}

export function sqlLintDiagnostics(query: string): Diagnostic[] {
	const diagnostics: Diagnostic[] = [];
	const lintRules = sqlRules.lint ?? {};
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
	if (lintRules.single_statement_only !== false) {
		if (semicolonIndex !== -1 && semicolonIndex < query.length - 1) {
			diagnostics.push({
				from: semicolonIndex,
				to: semicolonIndex + 1,
				severity: "warning",
				message: "Only a single statement is supported",
			});
		}
	}

	const limitMatch = /\blimit\b\s+([^\s;]+)/i.exec(query);
	if (lintRules.limit_requires_number !== false) {
		if (limitMatch && Number.isNaN(Number(limitMatch[1]))) {
			const from = limitMatch.index + limitMatch[0].indexOf(limitMatch[1]);
			diagnostics.push({
				from,
				to: from + limitMatch[1].length,
				severity: "error",
				message: "LIMIT value must be a number",
			});
		}
	}

	return diagnostics;
}
