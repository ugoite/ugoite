import type { Diagnostic } from "@codemirror/lint";
import type { Form, SqlVariable, SqlVariableDefault } from "./types";
import { lintSql } from "./ugoite-client";
import { normalizeSqlTemplate, parseSqlDocument, UgoiteApiError } from "./ugoite-client/protocol";
import type { SqlDocument, SqlLintProfile, SqlTemplateNormalization } from "./ugoite-client";

export type SqlSchema = { tables: Record<string, string[]> };

const sqlSchemaFormLabels = new WeakMap<object, Readonly<Record<string, string>>>();

export function sqlSchemaFormLabelsFor(schema: SqlSchema): Readonly<Record<string, string>> {
  return sqlSchemaFormLabels.get(schema as object) ?? {};
}

const SQL_SYSTEM_COLUMNS = ["_ugoite_id", "_ugoite_created_at", "_ugoite_updated_at"];

export function buildSqlSchema(forms: Form[]): SqlSchema {
  // Null-prototype storage: valid Form names include `__proto__`, which a
  // plain object literal would swallow as a prototype assignment instead of
  // an ordinary table entry.
  const tables: Record<string, string[]> = Object.create(null);
  const tableLabels: Record<string, string> = Object.create(null);
  for (const item of forms) {
    const name = item.sql_relation?.trim();
    if (!name) continue;
    const columns = Object.values(item.fields ?? {})
      .map((field) => field.sql_column?.trim())
      .filter((column): column is string => Boolean(column));
    tables[name] = [...SQL_SYSTEM_COLUMNS, ...columns];
    tableLabels[name] = item.name.trim() || name;
  }

  const schema = { tables };
  sqlSchemaFormLabels.set(schema, tableLabels);
  return schema;
}

function utf8ByteToUtf16Offsets(source: string): Map<number, number> {
  const offsets = new Map<number, number>([[0, 0]]);
  let byteOffset = 0;
  let utf16Offset = 0;
  for (const character of source) {
    const codePoint = character.codePointAt(0) ?? 0;
    byteOffset += codePoint <= 0x7f
      ? 1
      : codePoint <= 0x7ff
      ? 2
      : codePoint <= 0xffff
      ? 3
      : 4;
    utf16Offset += character.length;
    offsets.set(byteOffset, utf16Offset);
  }
  return offsets;
}

type SqlParameterReplacement = {
  name: string;
  start: number;
  end: number;
  normalizedStart: number;
  normalizedEnd: number;
  replacement: string;
};

type NormalizedSqlDocument = {
  source: string;
  sql: string;
  variables: SqlVariable[];
  document: SqlDocument;
  replacements: SqlParameterReplacement[];
};

export async function normalizeSqlDocument(
  sql: string,
  existingVariables: SqlVariable[] = [],
): Promise<NormalizedSqlDocument> {
  let normalized: SqlTemplateNormalization;
  try {
    normalized = await normalizeSqlTemplate(sql);
  } catch (error) {
    // The browser SQL Document limit is explicit; preserve its existing
    // TooLarge diagnostics and empty parameter discovery for oversized drafts.
    if (!(error instanceof UgoiteApiError) || error.kind !== "sql_template_normalization" || error.code !== "source_too_large") {
      throw error;
    }
    const document = await parseSqlDocument(sql);
    return { source: sql, sql, variables: [], document, replacements: [] };
  }
  const normalizedSql = normalized.sql;
  const sourceByteToUtf16 = utf8ByteToUtf16Offsets(sql);
  const normalizedByteToUtf16 = utf8ByteToUtf16Offsets(normalizedSql);
  const replacements: SqlParameterReplacement[] = [];
  let byteShift = 0;
  for (const replacement of normalized.replacements) {
    const [sourceStartByte, sourceEndByte, normalizedLengthByte] = replacement;
    const normalizedStartByte = sourceStartByte - byteShift;
    const normalizedEndByte = normalizedStartByte + normalizedLengthByte;
    const start = sourceByteToUtf16.get(sourceStartByte);
    const end = sourceByteToUtf16.get(sourceEndByte);
    const normalizedStart = normalizedByteToUtf16.get(normalizedStartByte);
    const normalizedEnd = normalizedByteToUtf16.get(normalizedEndByte);
    if (start !== undefined && end !== undefined && normalizedStart !== undefined && normalizedEnd !== undefined) {
      const replacementText = normalizedSql.slice(normalizedStart, normalizedEnd);
      replacements.push({
        name: replacementText.slice(1),
        start,
        end,
        normalizedStart,
        normalizedEnd,
        replacement: replacementText,
      });
    }
    byteShift += sourceEndByte - sourceStartByte - normalizedLengthByte;
  }
  const document = normalizedSql === sql
    ? await parseSqlDocument(sql)
    : await parseSqlDocument(normalizedSql);
  const existingByName = new Map(
    existingVariables.map((variable) => [variable.name, variable]),
  );
  return {
    source: sql,
    sql: normalizedSql,
    variables: normalized.parameters.map((name) =>
      existingByName.get(name) ?? {
        type: "string",
        name,
        description: "",
      }
    ),
    document,
    replacements,
  };
}

/** Normalize SQL parameter authoring syntax using the shared Rust lexer. */
export async function normalizeSqlVariables(
  sql: string,
  existingVariables: SqlVariable[] = [],
): Promise<{
  sql: string;
  variables: SqlVariable[];
}> {
  const normalized = await normalizeSqlDocument(sql, existingVariables);
  return { sql: normalized.sql, variables: normalized.variables };
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
      const significand = raw.split(/[eE]/, 1)[0];
      const underflowed = value === 0 && /[1-9]/.test(significand);
      return Number.isFinite(value) && !underflowed
        ? { valid: true, value }
        : { valid: false };
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

/** Convert portable Rust SQL lint diagnostics to CodeMirror's UTF-16 offsets. */
export async function sqlLintDiagnostics(
  query: string,
  profile: SqlLintProfile = "standard",
): Promise<Diagnostic[]> {
  const response = await lintSql(query, profile);
  const byteToUtf16 = utf8ByteToUtf16Offsets(query);
  return response.diagnostics.map((diagnostic) => {
    const from = diagnostic.range
      ? byteToUtf16.get(diagnostic.range.start)
      : undefined;
    const to = diagnostic.range
      ? byteToUtf16.get(diagnostic.range.end)
      : undefined;
    return {
      from: from ?? 0,
      to: to ?? query.length,
      severity: diagnostic.severity,
      message: diagnostic.message,
    };
  });
}
