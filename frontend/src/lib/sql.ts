import { StandardSQL } from "@codemirror/lang-sql";
import type { SQLConfig } from "@codemirror/lang-sql";
import type { Diagnostic } from "@codemirror/lint";
import sqlRules from "../../../shared/sql/ugoite-sql-rules.json";
import type { Form, SqlVariable } from "./types";

export type SqlSchema = NonNullable<SQLConfig["schema"]>;

export const SQL_SESSION_DEFAULT_LIMIT = 50;
export const SQL_SESSION_ORDER = "ORDER BY _ugoite_updated_at DESC, _ugoite_id";

const SQL_SYSTEM_COLUMNS = [
  "_ugoite_id",
  "_ugoite_title",
  "_ugoite_created_at",
  "_ugoite_updated_at",
];
export function buildSqlSchema(forms: Form[]): SqlSchema {
  const tables: Record<string, string[]> = {};
  for (const item of forms) {
    /* v8 ignore start */
    const relation = item.sql_relation?.trim();
    if (!relation) continue;
    const columns = Object.values(item.fields ?? {})
      .map((field) => field.sql_column?.trim())
      .filter((column): column is string => Boolean(column));
    tables[relation] = [...SQL_SYSTEM_COLUMNS, ...columns];
    /* v8 ignore stop */
  }

  return { tables };
}

function quoteSqlIdentifier(value: string): string {
  return `"${value.replaceAll('"', '""')}"`;
}

/** Build the runnable query shown when a user creates Saved SQL. */
export function buildSqlStarterQuery(sqlRelation: string): string {
  return `SELECT * FROM ${
    quoteSqlIdentifier(sqlRelation)
  } ${SQL_SESSION_ORDER} LIMIT ${SQL_SESSION_DEFAULT_LIMIT}`;
}

const SQL_LITERAL_OR_COMMENT_NODES = new Set([
  "BlockComment",
  "LineComment",
  "String",
]);
const SQL_QUOTED_IDENTIFIER_NODE = "QuotedIdentifier";

type ParsedSql = {
  code: string;
  variableCode: string;
};

function parseSql(sql: string): ParsedSql {
  const ignoredRanges: Array<[number, number]> = [];
  const quotedIdentifierRanges: Array<[number, number]> = [];
  const cursor = StandardSQL.language.parser.parse(sql).cursor();

  const visit = (): void => {
    if (SQL_LITERAL_OR_COMMENT_NODES.has(cursor.type.name)) {
      ignoredRanges.push([cursor.from, cursor.to]);
      return;
    }
    if (cursor.type.name === SQL_QUOTED_IDENTIFIER_NODE) {
      quotedIdentifierRanges.push([cursor.from, cursor.to]);
      return;
    }
    if (cursor.firstChild()) {
      do visit(); while (cursor.nextSibling());
      cursor.parent();
    }
  };
  if (cursor.firstChild()) {
    do {
      visit();
    } while (cursor.nextSibling());
    cursor.parent();
  }

  const maskRanges = (ranges: Array<[number, number]>): string => {
    const masked = sql.split("");
    for (const [from, to] of ranges) {
      for (let offset = from; offset < to; offset += 1) masked[offset] = " ";
    }
    return masked.join("");
  };
  const code = maskRanges(ignoredRanges);
  return {
    code,
    variableCode: maskRanges([...ignoredRanges, ...quotedIdentifierRanges]),
  };
}

/**
 * Normalize the editor's template variables to DataFusion placeholders.
 * CodeMirror's SQL parser keeps strings, quoted identifiers, and comments out
 * of the placeholder scan, so `$name` in user text is not mistaken for a
 * session parameter.
 */
export function normalizeSqlVariables(sql: string): {
  sql: string;
  variables: SqlVariable[];
} {
  const names = new Set<string>();
  const parsed = parseSql(sql);
  const variablePattern =
    /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}|\$([A-Za-z_][A-Za-z0-9_]*)/g;
  const normalizedSql = sql.replace(
    variablePattern,
    (
      match,
      templatedName: string | undefined,
      nativeName: string | undefined,
      offset: number,
    ) => {
      if (parsed.variableCode[offset] === " ") return match;
      if (nativeName && /[A-Za-z0-9_$]/.test(sql[offset - 1] ?? "")) {
        return match;
      }
      const name = templatedName ?? nativeName;
      if (!name) return match;
      names.add(name);
      return `$${name}`;
    },
  );

  return {
    sql: normalizedSql,
    variables: Array.from(names).map((name) => ({
      type: "string",
      name,
      description: "",
    })),
  };
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

  const parsedQuery = parseSql(query);
  const selectMatch = /\bselect\b/i.exec(parsedQuery.code);
  if (lintRules.require_select !== false && !selectMatch) {
    diagnostics.push({
      from: leadingWhitespace,
      to: leadingWhitespace + Math.min(trimmed.length, 6),
      severity: "error",
      message: "Read-only query must start with SELECT",
    });
  }

  const fromMatch = /\bfrom\b/i.exec(parsedQuery.code);
  if (lintRules.require_from !== false && !fromMatch) {
    diagnostics.push({
      from: Math.max(0, query.length - 1),
      to: query.length,
      severity: "error",
      message: "Read-only query must include FROM",
    });
  }

  const semicolonIndex = parsedQuery.code.indexOf(";");
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

  const limitMatch = /\blimit\b\s+([^\s;]+)/i.exec(parsedQuery.code);
  /* v8 ignore start */
  if (lintRules.limit_requires_number !== false && limitMatch) {
    /* v8 ignore stop */
    const limitValue = limitMatch[1];
    const isPlaceholder = /^\$[A-Za-z_][A-Za-z0-9_]*$/.test(limitValue) ||
      /^\{\{\s*[A-Za-z_][A-Za-z0-9_]*\s*\}\}$/.test(limitValue);
    if (Number.isNaN(Number(limitValue)) && !isPlaceholder) {
      const from = limitMatch.index + limitMatch[0].indexOf(limitValue);
      diagnostics.push({
        from,
        to: from + limitValue.length,
        severity: "warning",
        message: "LIMIT value should be a number or parameter",
      });
    }
    /* v8 ignore start */
  }
  /* v8 ignore stop */

  return diagnostics;
}
