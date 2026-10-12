import { StandardSQL } from "@codemirror/lang-sql";
import type { SQLConfig } from "@codemirror/lang-sql";
import type { Diagnostic } from "@codemirror/lint";
import sqlRules from "../../../shared/sql/ugoite-sql-rules.json";
import type { Form, FormField, SqlVariable } from "./types";

export type SqlSchema = NonNullable<SQLConfig["schema"]>;

export const SQL_QUERY_DEFAULT_LIMIT = 50;
export const SQL_QUERY_ORDER = "ORDER BY _ugoite_updated_at DESC, _ugoite_id";

const SQL_SYSTEM_COLUMNS = [
  "_ugoite_id",
  "_ugoite_created_at",
  "_ugoite_updated_at",
];
export function buildSqlSchema(forms: Form[]): SqlSchema {
  // Null-prototype storage: valid Form names include `__proto__`, which a
  // plain object literal would swallow as a prototype assignment instead of
  // an ordinary table entry.
  const tables: Record<string, string[]> = Object.create(null);
  for (const item of forms) {
    const name = item.name.trim();
    if (!name) continue;
    const columns = Object.values(item.fields ?? {})
      .map((field) => field.sql_column?.trim())
      .filter((column): column is string => Boolean(column));
    tables[name] = [...SQL_SYSTEM_COLUMNS, ...columns];
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
  } ${SQL_QUERY_ORDER} LIMIT ${SQL_QUERY_DEFAULT_LIMIT}`;
}

export type SqlAggregateOperation =
  | "count-rows"
  | "count-values"
  | "sum"
  | "average"
  | "minimum"
  | "maximum";

export type SqlBuilderField = {
  key: string;
  label: string;
  column: string;
  fieldType: string;
  queryCapabilitySortable?: boolean;
  queryCapabilitySupportedOperators?: string[];
};

export type SqlBuilderParameter = {
  name: "filter_value";
  type: string;
  description: string;
};

export type SqlAggregateQuery = {
  sql: string;
  parameter?: SqlBuilderParameter;
};

const numericFieldTypes = new Set(["integer", "long", "float", "double"]);
const orderableFieldTypes = new Set([
  ...numericFieldTypes,
  "date",
  "timestamp",
  "timestamp_tz",
  "timestamp_ns",
  "timestamp_tz_ns",
]);

// Keep filters on parameter types shared by ordinary Saved SQL and
// Composition. long is omitted because its input currently passes through
// JavaScript's imprecise Number type; zoned/nanosecond timestamps are omitted
// because Composition parameters use the portable timestamp type.
const sqlVariableTypes = new Map<string, string>([
  ["string", "string"],
  ["markdown", "string"],
  ["sql", "string"],
  ["boolean", "boolean"],
  ["integer", "integer"],
  ["float", "float"],
  ["double", "float"],
  ["date", "date"],
  ["timestamp", "timestamp"],
]);

const generatedOrdinal = /^(?:field|フィールド|項目)[\s_-]*\d+$/iu;
const opaqueIdentifier =
  /^(?:\d+|[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})$/iu;

const displaySqlBuilderFieldName = (
  key: string,
  field: FormField,
): string | undefined => {
  const candidates = [field.label, field.query_capability?.name, key];
  return candidates.map((value) => value?.trim()).find((value) =>
    value && !generatedOrdinal.test(value) && !opaqueIdentifier.test(value)
  );
};

const sqlBuilderFieldsFor = (form: Form | undefined): SqlBuilderField[] => {
  if (!form) return [];
  return Object.entries(form.fields ?? {}).flatMap(([key, field]) => {
    const column = field.sql_column?.trim();
    const label = displaySqlBuilderFieldName(key, field);
    if (!column || !label || field.deprecated) return [];
    if (field.query_capability?.projectable === false) return [];
    return [{
      key,
      label,
      column,
      fieldType: field.query_capability?.field_type ?? field.type,
      queryCapabilitySortable: field.query_capability?.sortable,
      queryCapabilitySupportedOperators:
        field.query_capability?.supported_operators,
    }];
  });
};

/** Return the current Form fields compatible with the selected aggregate. */
export const sqlAggregateFieldOptions = (
  form: Form | undefined,
  operation: SqlAggregateOperation,
): SqlBuilderField[] => {
  const fields = sqlBuilderFieldsFor(form);
  if (operation === "count-rows") return [];
  if (operation === "count-values") return fields;
  if (operation === "sum" || operation === "average") {
    return fields.filter((field) => numericFieldTypes.has(field.fieldType));
  }
  return fields.filter((field) =>
    orderableFieldTypes.has(field.fieldType) &&
    field.queryCapabilitySortable !== false
  );
};

const queryableSqlBuilderFields = (
  form: Form | undefined,
): SqlBuilderField[] =>
  sqlBuilderFieldsFor(form).filter((field) => {
    if (!sqlVariableTypes.has(field.fieldType)) return false;
    return !field.queryCapabilitySupportedOperators ||
      field.queryCapabilitySupportedOperators.includes("equals");
  });

/** Return fields that support equality filters with ordinary typed SQL parameters. */
export const sqlFilterFieldOptions = (
  form: Form | undefined,
): Array<SqlBuilderField & { parameterType: string }> =>
  queryableSqlBuilderFields(form).flatMap((field) => {
    const parameterType = sqlVariableTypes.get(field.fieldType);
    return parameterType ? [{ ...field, parameterType }] : [];
  });

/** Generate one-column ordinary SQL and an optional typed filter parameter. */
export const buildSqlAggregateQuery = (
  form: Form,
  operation: SqlAggregateOperation,
  aggregateFieldKey = "",
  filterFieldKey = "",
): SqlAggregateQuery => {
  const sourceName = form.name.trim();
  if (!sourceName) throw new Error("A Form is required");

  const aggregateField = operation === "count-rows"
    ? undefined
    : sqlAggregateFieldOptions(form, operation).find((field) =>
      field.key === aggregateFieldKey
    );
  if (operation !== "count-rows" && !aggregateField) {
    throw new Error("The aggregate field is unavailable");
  }

  const expression = operation === "count-rows"
    ? "COUNT(*)"
    : operation === "count-values"
    ? `COUNT(${quoteSqlIdentifier(aggregateField!.column)})`
    : operation === "sum"
    ? `SUM(${quoteSqlIdentifier(aggregateField!.column)})`
    : operation === "average"
    ? `AVG(${quoteSqlIdentifier(aggregateField!.column)})`
    : operation === "minimum"
    ? `MIN(${quoteSqlIdentifier(aggregateField!.column)})`
    : `MAX(${quoteSqlIdentifier(aggregateField!.column)})`;

  const filterField = filterFieldKey
    ? sqlFilterFieldOptions(form).find((field) => field.key === filterFieldKey)
    : undefined;
  if (filterFieldKey && !filterField) {
    throw new Error("The filter field is unavailable");
  }

  return {
    sql: `SELECT ${expression} AS "value" FROM ${quoteSqlIdentifier(sourceName)}${filterField
      ? ` WHERE ${quoteSqlIdentifier(filterField.column)} = {{filter_value}}`
      : ""}`,
    ...(filterField
      ? {
        parameter: {
          name: "filter_value",
          type: filterField.parameterType,
          description: filterField.label,
        } satisfies SqlBuilderParameter,
      }
      : {}),
  };
};

const SQL_LITERAL_OR_COMMENT_NODES = new Set([
  "BlockComment",
  "LineComment",
  "String",
]);
const SQL_QUOTED_IDENTIFIER_NODE = "QuotedIdentifier";
const SQL_STATEMENT_FORM_KEYWORDS = new Set([
  "alter",
  "create",
  "delete",
  "drop",
  "explain",
  "insert",
  "merge",
  "select",
  "truncate",
  "update",
]);

type ParsedSql = {
  code: string;
  statementForm: "select" | "non-select" | undefined;
  statementKeywords: Set<string>;
  variableCode: string;
};

type SqlTreeCursor = {
  type: { name: string };
  from: number;
  to: number;
  firstChild(): boolean;
  nextSibling(): boolean;
  parent(): boolean;
};

type StatementSummary = {
  form: "select" | "non-select";
  keywords: Set<string>;
};

function readStatementSummary(
  sql: string,
  cursor: SqlTreeCursor,
): StatementSummary | null {
  if (!cursor.firstChild()) return null;

  const keywords = new Set<string>();
  let firstToken: string | null = null;
  let statementFormToken: string | null = null;
  do {
    const nodeName = cursor.type.name;
    if (SQL_LITERAL_OR_COMMENT_NODES.has(nodeName) || nodeName === ";") {
      continue;
    }

    if (nodeName === "Keyword") {
      const token = sql.slice(cursor.from, cursor.to).toLowerCase();
      keywords.add(token);
      if (firstToken === null) {
        firstToken = token;
      }
      if (
        statementFormToken === null &&
        SQL_STATEMENT_FORM_KEYWORDS.has(token) &&
        token !== "with"
      ) {
        // For a CTE, the statement's actual form is the first statement-form
        // keyword after WITH. This keeps INSERT ... SELECT classified as
        // INSERT while allowing WITH ... SELECT queries.
        statementFormToken = token;
      }
    } else if (firstToken === null) {
      // Identifiers, literals, operators, and parser errors cannot start a
      // read-only query. The browser still only provides advisory feedback.
      firstToken = "";
    }
  } while (cursor.nextSibling());
  cursor.parent();

  if (firstToken === null) return null;
  return {
    form: firstToken === "select" ||
        (firstToken === "with" && statementFormToken === "select")
      ? "select"
      : "non-select",
    keywords,
  };
}

function readFirstStatementSummary(
  sql: string,
  cursor: SqlTreeCursor,
): StatementSummary | null {
  if (!cursor.firstChild()) return null;
  do {
    if (cursor.type.name === "Statement") {
      const summary = readStatementSummary(sql, cursor);
      if (summary) {
        cursor.parent();
        return summary;
      }
    }
  } while (cursor.nextSibling());
  cursor.parent();
  return null;
}

function parseSql(sql: string): ParsedSql {
  const ignoredRanges: Array<[number, number]> = [];
  const quotedIdentifierRanges: Array<[number, number]> = [];
  const tree = StandardSQL.language.parser.parse(sql);
  const cursor = tree.cursor();
  const statementSummary = readFirstStatementSummary(sql, tree.cursor());

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
  const code = maskRanges([...ignoredRanges, ...quotedIdentifierRanges]);
  return {
    code,
    statementForm: statementSummary?.form,
    statementKeywords: statementSummary?.keywords ?? new Set<string>(),
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
  if (
    lintRules.require_select !== false &&
    parsedQuery.statementForm !== "select"
  ) {
    diagnostics.push({
      from: leadingWhitespace,
      to: leadingWhitespace + Math.min(trimmed.length, 6),
      severity: "error",
      message: "Read-only query must start with SELECT",
    });
  }

  if (
    lintRules.require_from !== false &&
    !parsedQuery.statementKeywords.has("from")
  ) {
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
    if (semicolonIndex !== -1) {
      // `code` masks string literals, comments, and quoted identifiers, so a
      // `;` found here is a real statement delimiter. Trailing trivia
      // (whitespace and line/block comments, already masked to spaces) after
      // the final semicolon is not a second statement.
      const trailing = parsedQuery.code.slice(semicolonIndex + 1).trim();
      if (trailing.length > 0) {
        diagnostics.push({
          from: semicolonIndex,
          to: semicolonIndex + 1,
          severity: "warning",
          message: "Only a single statement is supported",
        });
      }
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
