import type { Diagnostic } from "@codemirror/lint";
import type { SQLConfig } from "@codemirror/lang-sql";
import type { Class } from "./types";

export type SqlSchema = NonNullable<SQLConfig["schema"]>;

const BASE_COLUMNS = ["id", "title", "class", "updated_at", "workspace_id", "word_count", "tags"];

export function buildSqlSchema(classes: Class[]): SqlSchema {
	const classFieldSet = new Set<string>();
	for (const item of classes) {
		for (const field of Object.keys(item.fields ?? {})) {
			classFieldSet.add(field);
		}
	}

	const unionFields = [...BASE_COLUMNS, ...classFieldSet];
	const tables: Record<string, string[]> = {
		notes: unionFields,
	};

	for (const item of classes) {
		tables[item.name] = [...BASE_COLUMNS, ...Object.keys(item.fields ?? {})];
	}

	return { tables };
}

export function sqlLintDiagnostics(query: string): Diagnostic[] {
	const diagnostics: Diagnostic[] = [];
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
	if (!selectMatch) {
		diagnostics.push({
			from: leadingWhitespace,
			to: leadingWhitespace + Math.min(trimmed.length, 6),
			severity: "error",
			message: "Query must start with SELECT",
		});
	}

	const fromMatch = /\bfrom\b/i.exec(trimmed);
	if (!fromMatch) {
		diagnostics.push({
			from: Math.max(0, query.length - 1),
			to: query.length,
			severity: "error",
			message: "Query must include FROM",
		});
	}

	const semicolonIndex = query.indexOf(";");
	if (semicolonIndex !== -1 && semicolonIndex < query.length - 1) {
		diagnostics.push({
			from: semicolonIndex,
			to: semicolonIndex + 1,
			severity: "warning",
			message: "Only a single statement is supported",
		});
	}

	const limitMatch = /\blimit\b\s+([^\s;]+)/i.exec(query);
	if (limitMatch && Number.isNaN(Number(limitMatch[1]))) {
		const from = limitMatch.index + limitMatch[0].indexOf(limitMatch[1]);
		diagnostics.push({
			from,
			to: from + limitMatch[1].length,
			severity: "error",
			message: "LIMIT value must be a number",
		});
	}

	return diagnostics;
}
