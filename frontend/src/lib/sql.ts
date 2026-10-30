import type { SQLConfig } from "@codemirror/lang-sql";
import type { Diagnostic } from "@codemirror/lint";
import { isReservedMetadataForm } from "./metadata-forms";
import type { Form, SqlVariable, SqlVariableDefault } from "./types";
import { normalizeSqlTemplate, parseSqlDocument, UgoiteApiError } from "./ugoite-client/protocol";
import type {
  SqlDocument,
  SqlDocumentDiagnostic,
  SqlStructureColumnType,
  SqlStructureContext,
  SqlStructureVariableType,
  SqlTemplateNormalization,
} from "./ugoite-client";

type SqlStructureAggregateFunction =
  | "count"
  | "sum"
  | "avg"
  | "mean"
  | "min"
  | "max";

export type SqlStructureFilterOperator =
  | "eq"
  | "not_eq"
  | "lt"
  | "lt_eq"
  | "gt"
  | "gt_eq"
  | "in"
  | "not_in"
  | "between"
  | "not_between"
  | "like"
  | "not_like"
  | "ilike"
  | "not_ilike"
  | "is_null"
  | "is_not_null";

export type SqlSchema = NonNullable<SQLConfig["schema"]>;

const sqlSchemaFormLabels = new WeakMap<
  object,
  Readonly<Record<string, string>>
>();

export function sqlSchemaFormLabelsFor(
  schema: SqlSchema,
): Readonly<Record<string, string>> {
  return sqlSchemaFormLabels.get(schema as object) ?? {};
}

const SQL_SYSTEM_COLUMNS = [
  "_ugoite_id",
  "_ugoite_created_at",
  "_ugoite_updated_at",
];

const sqlStructureColumnType = (value: string): SqlStructureColumnType => {
  switch (value) {
    case "string":
    case "markdown":
    case "sql":
    case "uuid":
      return "string";
    case "boolean":
      return "boolean";
    case "integer":
      return "integer";
    case "float":
    case "double":
      return "float";
    case "date":
      return "date";
    case "timestamp":
      return "timestamp";
    case "decimal":
      return "decimal";
    case "json":
    case "list":
    case "object_list":
    case "row_reference":
    case "asset_reference":
      return "json";
    default:
      return "other";
  }
};

const sqlStructureVariableType = (
  value: string,
): SqlStructureVariableType | undefined => {
  switch (value) {
    case "string":
    case "boolean":
    case "integer":
    case "float":
    case "date":
    case "timestamp":
      return value;
    default:
      return undefined;
  }
};

const aggregateFunctionsForType = (
  type: SqlStructureColumnType,
): SqlStructureAggregateFunction[] => {
  switch (type) {
    case "integer":
    case "float":
    case "decimal":
      return ["count", "sum", "avg", "mean", "min", "max"];
    case "string":
    case "boolean":
    case "date":
    case "timestamp":
      return ["count", "min", "max"];
    case "json":
      return ["count"];
    case "other":
      return ["count"];
  }
};

const sqlStructureTypeSupportsGrouping = (
  type: SqlStructureColumnType,
): boolean => type !== "json" && type !== "other";

const sqlStructureTypeSupportsDistinct = (
  type: SqlStructureColumnType,
): boolean => type !== "json" && type !== "other";

export interface SqlStructureColumnCapabilities {
  projectable: boolean;
  filterable: boolean;
  sortable: boolean;
  supportedOperators: readonly SqlStructureFilterOperator[];
  aggregateFunctions: readonly SqlStructureAggregateFunction[];
  groupable: boolean;
  distinctable: boolean;
}

/** Typed parser input plus the server-derived per-column action contract. */
export interface SqlStructureAdapterContext {
  /** Identity of the source and schema context that produced this adapter context. */
  identity: string;
  /** Whether the Form relation may be offered by the relation chooser. */
  relationSelectable: boolean;
  /** Human-readable Form name authored as the SQL relation. */
  label?: string;
  /** Stable Form identity used only as an expected-identity guard. */
  formId?: string;
  /** Stable executor relation retained for schema/context diagnostics. */
  stableRelation?: string;
  parser: SqlStructureContext;
  columnCapabilities: Readonly<Record<string, SqlStructureColumnCapabilities>>;
  /** Human field names keyed by immutable SQL column identifiers. */
  columnLabels?: Readonly<Record<string, string>>;
}

/** Hide stale same-name schema contexts while a selected Form ID is pinned. */
export function filterSqlStructureContextsByFormGuards(
  contexts: readonly SqlStructureAdapterContext[],
  guards: readonly { name: string; formId: string }[],
): SqlStructureAdapterContext[] {
  return contexts.filter((context) => {
    const guard = guards.find((candidate) =>
      candidate.name === context.parser.relation
    );
    return !guard || context.formId === guard.formId;
  });
}

const queryOperators = (
  supported: readonly string[] | undefined,
  filterable: boolean,
  columnType: SqlStructureColumnType,
): SqlStructureFilterOperator[] => {
  const mapping: Record<string, SqlStructureFilterOperator> = {
    equals: "eq",
    lt: "lt",
    lte: "lt_eq",
    gt: "gt",
    gte: "gt_eq",
  };
  const operators = new Set<SqlStructureFilterOperator>([
    ...(supported ?? []).flatMap((operator) =>
      mapping[operator] ? [mapping[operator]] : []
    ),
  ]);
  if (filterable) {
    const scalarParameter = [
      "string",
      "boolean",
      "integer",
      "float",
      "date",
      "timestamp",
    ].includes(columnType);
    if (scalarParameter) {
      operators.add("in");
      operators.add("not_in");
      if (columnType !== "boolean") {
        operators.add("between");
        operators.add("not_between");
      }
    }
    if (columnType === "string") {
      operators.add("like");
      operators.add("not_like");
      operators.add("ilike");
      operators.add("not_ilike");
    }
    operators.add("is_null");
    operators.add("is_not_null");
  }
  return [...operators];
};

/** Build analysis contexts for every Form plus a relation-free SQL context. */
export const buildSqlStructureContexts = (
  forms: readonly Form[],
  variables: readonly { name: string; type: string }[] = [],
  sourceIdentity = "sql",
): SqlStructureAdapterContext[] => {
  const structureVariables = variables.flatMap((variable) => {
    const type = sqlStructureVariableType(variable.type);
    return type ? [{ name: variable.name, type }] : [];
  });

  const formContexts = forms.flatMap((form) => {
    const stableRelation = form.sql_relation?.trim();
    const relation = form.name?.trim();
    if (!stableRelation || !relation || !form.id) return [];
    const columns = [
      { name: "_ugoite_id", type: "string" as const },
      { name: "_ugoite_created_at", type: "timestamp" as const },
      { name: "_ugoite_updated_at", type: "timestamp" as const },
      ...Object.values(form.fields ?? {}).flatMap((field) => {
        const name = field.sql_column?.trim();
        if (!name) return [];
        const type = sqlStructureColumnType(
          field.query_capability?.field_type ?? field.type,
        );
        return [{ name, type }];
      }),
    ];
    const columnLabels: Record<string, string> = Object.fromEntries(
      Object.entries(form.fields ?? {}).flatMap(([label, field]) =>
        field.sql_column?.trim()
          ? [[field.sql_column.trim(), field.label?.trim() || label]]
          : []
      ),
    );
    const columnCapabilities: Record<string, SqlStructureColumnCapabilities> = {
      _ugoite_id: {
        projectable: true,
        filterable: true,
        sortable: true,
        supportedOperators: queryOperators(
          ["equals"],
          true,
          "string",
        ),
        aggregateFunctions: aggregateFunctionsForType("string"),
        groupable: true,
        distinctable: true,
      },
      _ugoite_created_at: {
        projectable: true,
        filterable: true,
        sortable: true,
        supportedOperators: queryOperators(
          ["equals", "lt", "lte", "gt", "gte"],
          true,
          "timestamp",
        ),
        aggregateFunctions: aggregateFunctionsForType("timestamp"),
        groupable: true,
        distinctable: true,
      },
      _ugoite_updated_at: {
        projectable: true,
        filterable: true,
        sortable: true,
        supportedOperators: queryOperators(
          ["equals", "lt", "lte", "gt", "gte"],
          true,
          "timestamp",
        ),
        aggregateFunctions: aggregateFunctionsForType("timestamp"),
        groupable: true,
        distinctable: true,
      },
    };
    for (const field of Object.values(form.fields ?? {})) {
      const name = field.sql_column?.trim();
      if (!name) continue;
      const capability = field.query_capability;
      const queryType = capability
        ? sqlStructureColumnType(capability.field_type)
        : undefined;
      const projectable = capability?.projectable === true;
      columnCapabilities[name] = {
        projectable,
        filterable: capability?.filterable === true,
        sortable: capability?.sortable === true,
        supportedOperators: queryOperators(
          capability?.supported_operators,
          capability?.filterable === true,
          queryType ?? "other",
        ),
        aggregateFunctions: projectable && queryType
          ? aggregateFunctionsForType(queryType)
          : [],
        groupable: projectable && queryType !== undefined &&
          sqlStructureTypeSupportsGrouping(queryType),
        distinctable: projectable && queryType !== undefined &&
          sqlStructureTypeSupportsDistinct(queryType),
      };
    }
    const parser: SqlStructureContext = {
      relation,
      columns,
      variables: structureVariables,
      stable_key: ["_ugoite_id"],
    };
    return [{
      identity: JSON.stringify({
        sourceIdentity,
        formId: form.id ?? null,
        formName: form.name,
        formVersion: form.version,
        relation: stableRelation,
        relationSelectable: !isReservedMetadataForm(form.name),
        parser,
        columnCapabilities,
        columnLabels,
      }),
      relationSelectable: !isReservedMetadataForm(form.name),
      label: form.name.trim() || relation,
      formId: form.id,
      stableRelation,
      parser,
      columnCapabilities,
      columnLabels,
    }];
  });
  const parser: SqlStructureContext = {
    relation: "",
    columns: [],
    variables: structureVariables,
    stable_key: [],
  };
  return [
    ...formContexts,
    {
      identity: JSON.stringify({ sourceIdentity, relation: null, parser }),
      relationSelectable: false,
      parser,
      columnCapabilities: {},
    },
  ];
};

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

export function mergeSqlStructureVariables(
  existing: readonly SqlVariable[],
  added: readonly { name: string; type: SqlStructureVariableType }[],
): SqlVariable[] {
  const byName = new Map(existing.map((variable) => [variable.name, variable]));
  for (const { name, type } of added) {
    const current = byName.get(name);
    if (!current || current.type !== type) {
      byName.set(name, { name, type, description: "" });
    }
  }
  return [...byName.values()];
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

function diagnosticForSqlDocument(
  normalized: NormalizedSqlDocument,
  sourceDiagnostic: SqlDocumentDiagnostic,
): Diagnostic {
  const byteToUtf16 = utf8ByteToUtf16Offsets(normalized.sql);
  const normalizedStart = sourceDiagnostic.range
    ? byteToUtf16.get(sourceDiagnostic.range.start)
    : undefined;
  const normalizedEnd = sourceDiagnostic.range
    ? byteToUtf16.get(sourceDiagnostic.range.end)
    : undefined;
  const toSourceOffset = (offset: number): number => {
    let shift = 0;
    for (const replacement of normalized.replacements) {
      if (offset <= replacement.normalizedStart) return offset + shift;
      if (offset <= replacement.normalizedEnd) {
        return offset === replacement.normalizedEnd
          ? replacement.end
          : replacement.start;
      }
      shift += replacement.end - replacement.start -
        (replacement.normalizedEnd - replacement.normalizedStart);
    }
    return offset + shift;
  };
  const start = normalizedStart === undefined
    ? undefined
    : toSourceOffset(normalizedStart);
  const end = normalizedEnd === undefined
    ? undefined
    : toSourceOffset(normalizedEnd);
  const severity = sourceDiagnostic.code === "single_statement_required"
    ? "warning"
    : "error";
  const message = sourceDiagnostic.code === "empty_document"
    ? "Query is required"
    : sourceDiagnostic.code === "select_required"
    ? "Read-only query must start with SELECT"
    : sourceDiagnostic.code === "single_statement_required"
    ? "Only a single statement is supported"
    : sourceDiagnostic.message;
  const leadingWhitespace = normalized.source.length -
    normalized.source.trimStart().length;
  return {
    from: start ?? leadingWhitespace,
    to: end ?? Math.min(normalized.source.length, leadingWhitespace + 1),
    severity,
    message,
  };
}

/** Convert Rust SQL Document diagnostics to CodeMirror's display shape. */
export async function sqlLintDiagnostics(query: string): Promise<Diagnostic[]> {
  const normalized = await normalizeSqlDocument(query);
  return normalized.document.diagnostics.map((diagnostic) =>
    diagnosticForSqlDocument(normalized, diagnostic)
  );
}
