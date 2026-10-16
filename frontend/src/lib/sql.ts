import { StandardSQL } from "@codemirror/lang-sql";
import type { SQLConfig } from "@codemirror/lang-sql";
import type { Diagnostic } from "@codemirror/lint";
import sqlRules from "../../../shared/sql/ugoite-sql-rules.json";
import type { Form, FormField, SqlVariable, SqlVariableDefault } from "./types";

export type SqlSchema = NonNullable<SQLConfig["schema"]>;

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

/** Build an initial relation query with stable ordering and request paging. */
export function buildSqlStarterQuery(sqlRelation: string): string {
  return `SELECT * FROM ${quoteSqlIdentifier(sqlRelation)} ORDER BY _ugoite_updated_at DESC, _ugoite_id`;
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
      queryCapabilitySupportedOperators: field.query_capability
        ?.supported_operators,
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
    sql: `SELECT ${expression} AS "value" FROM ${
      quoteSqlIdentifier(sourceName)
    }${
      filterField
        ? ` WHERE ${quoteSqlIdentifier(filterField.column)} = {{filter_value}}`
        : ""
    }`,
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
export function normalizeSqlVariables(
  sql: string,
  existingVariables: SqlVariable[] = [],
): {
  sql: string;
  variables: SqlVariable[];
} {
  const names = new Set<string>();
  const existingByName = new Map(
    existingVariables.map((variable) => [variable.name, variable]),
  );
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
    variables: Array.from(names).map((name) =>
      existingByName.get(name) ?? {
        type: "string",
        name,
        description: "",
      }
    ),
  };
}

export interface SqlVariableDefaultDraft {
  enabled: boolean;
  raw: string;
}

export type SqlVariableDefaultParseResult =
  | { valid: true; value: SqlVariableDefault }
  | { valid: false };

export function sqlVariableDefaultInputValue(
  value: SqlVariableDefault | undefined,
): string {
  return value === undefined ? "" : String(value);
}

export function sqlVariableDefaultDraft(
  variable: Pick<SqlVariable, "default">,
): SqlVariableDefaultDraft {
  return {
    enabled: variable.default !== undefined,
    raw: sqlVariableDefaultInputValue(variable.default),
  };
}

export function parseSqlVariableDefault(
  type: string,
  raw: string,
): SqlVariableDefaultParseResult {
  switch (type) {
    case "string":
      return isValidSqlVariableString(raw)
        ? { valid: true, value: raw }
        : { valid: false };
    case "boolean":
      return raw === "true" || raw === "false"
        ? { valid: true, value: raw === "true" }
        : { valid: false };
    case "integer": {
      if (raw.length > 17 || !/^[+-]?\d+$/.test(raw)) return { valid: false };
      const value = Number(raw);
      return Number.isSafeInteger(value)
        ? { valid: true, value }
        : { valid: false };
    }
    case "float": {
      if (
        raw.length > 64 ||
        !/^[+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(raw)
      ) return { valid: false };
      const value = Number(raw);
      return Number.isFinite(value) ? { valid: true, value } : { valid: false };
    }
    case "date":
      return isValidSqlVariableDate(raw)
        ? { valid: true, value: raw }
        : { valid: false };
    case "timestamp":
      return isValidSqlVariableTimestamp(raw)
        ? { valid: true, value: raw }
        : { valid: false };
    default:
      return { valid: false };
  }
}

export function applySqlVariableDefaultDrafts(
  variables: SqlVariable[],
  drafts: Record<string, SqlVariableDefaultDraft>,
): { variables: SqlVariable[]; invalid?: Pick<SqlVariable, "name" | "type"> } {
  const normalized: SqlVariable[] = [];
  for (const variable of variables) {
    const draft = drafts[variable.name] ?? sqlVariableDefaultDraft(variable);
    if (!draft.enabled) {
      const { default: _default, ...withoutDefault } = variable;
      normalized.push(withoutDefault);
      continue;
    }
    const parsed = parseSqlVariableDefault(variable.type, draft.raw);
    if (!parsed.valid) {
      return {
        variables: [],
        invalid: { name: variable.name, type: variable.type },
      };
    }
    normalized.push({ ...variable, default: parsed.value });
  }
  return { variables: normalized };
}

function isValidSqlVariableString(value: string): boolean {
  if (new TextEncoder().encode(value).length > 4096) return false;
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if (
      (code <= 0x1f || (code >= 0x7f && code <= 0x9f)) &&
      code !== 0x09 && code !== 0x0a && code !== 0x0d
    ) return false;
  }
  return true;
}

function isValidSqlVariableDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1 || year > 9999 || month < 1 || month > 12) return false;
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const monthDays = [
    31,
    leapYear ? 29 : 28,
    31,
    30,
    31,
    30,
    31,
    31,
    30,
    31,
    30,
    31,
  ];
  return day >= 1 && day <= monthDays[month - 1];
}

function isValidSqlVariableTimestamp(value: string): boolean {
  if (value.length < 20 || value.length > 35) {
    return false;
  }
  const match =
    /^(\d{4}-\d{2}-\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?(Z|[+-]\d{2}:\d{2})$/
      .exec(
        value,
      );
  if (!match || !isValidSqlVariableDate(match[1])) return false;
  if (
    Number(match[2]) > 23 ||
    Number(match[3]) > 59 ||
    Number(match[4]) > 59
  ) return false;
  if (match[6] === "-00:00") return false;
  if (match[6] !== "Z") {
    const hours = Number(match[6].slice(1, 3));
    const minutes = Number(match[6].slice(4, 6));
    if (hours > 23 || minutes > 59) return false;
  }
  return true;
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
