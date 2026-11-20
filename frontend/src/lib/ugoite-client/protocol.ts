import initializeWasm from "../generated/ugoite_wasm.wasm?init";
import { apiFetch, type ApiFetchOptions } from "../api";
import type { AssetReference } from "../types";
import {
  decodeCompositionDocumentIntegers,
  decodeCompositionEvaluationIntegers,
  decodeCompositionPreviewIntegerDefaults,
  encodeCompositionDocumentIntegers,
  encodeMarkedIntegerVariableValues,
  encodeVariableGraphIntegers,
  exactIntegerSourceValue,
  parseJsonWithNumberSources,
} from "../exact-integer";
import {
  classifyMutationOutcome,
  MUTATION_ERROR_CODES,
  type MutationOutcome,
} from "../mutation-outcome";

import type { UgoiteApiOperation } from "../generated/operation-registry";

export {
  UGOITE_API_OPERATIONS,
  type UgoiteApiOperation,
} from "../generated/operation-registry";

type ProtocolHeader = {
  name: string;
  value: string;
};

type PreparedRequest = {
  operation: UgoiteApiOperation;
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  path: string;
  headers: ProtocolHeader[];
  body?: string;
  body_kind: "none" | "json" | "multipart";
};

type ProtocolErrorPayload<TCode extends string = string> = {
  kind: string;
  message: string;
  code?: TCode;
  contract_version?: number;
  diagnostics?: VariableGraphDiagnostic[];
  operation?: string;
  status?: number;
  detail?: unknown;
  payload?: unknown;
};

type ProtocolEnvelope<T, TCode extends string = string> =
  | { ok: true; value: T }
  | { ok: false; error: ProtocolErrorPayload<TCode> };

type UgoiteWasmExports = WebAssembly.Exports & {
  memory: WebAssembly.Memory;
  ugoite_protocol_version(): number;
  ugoite_alloc(length: number): number;
  ugoite_dealloc(pointer: number, length: number): void;
  ugoite_protocol_invoke(pointer: number, length: number): number;
  ugoite_protocol_result_pointer(): number;
  ugoite_protocol_result_length(): number;
  ugoite_protocol_clear_result(): void;
};

export const UGOITE_WASM_PROTOCOL_VERSION = 1;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();
let exportsPromise: Promise<UgoiteWasmExports> | undefined;
let loadedExports: UgoiteWasmExports | undefined;

const loadExports = async (): Promise<UgoiteWasmExports> => {
  exportsPromise ??= initializeWasm().then((instance) => {
    const exports = instance.exports as UgoiteWasmExports;
    const protocolVersion = exports.ugoite_protocol_version();
    if (protocolVersion !== UGOITE_WASM_PROTOCOL_VERSION) {
      throw new Error(
        `Unsupported Ugoite WASM protocol version: ${protocolVersion}`,
      );
    }
    loadedExports = exports;
    return exports;
  });
  return await exportsPromise;
};

type ProtocolCommand = Record<string, unknown>;

const invokeProtocolWithExports = <T, TCode extends string = string>(
  exports: UgoiteWasmExports,
  command: ProtocolCommand,
): T => {
  const encodedCommand = encodeTypedIntegerCommand(command);
  const input = textEncoder.encode(JSON.stringify(encodedCommand));
  const inputPointer = exports.ugoite_alloc(input.byteLength);

  try {
    new Uint8Array(
      exports.memory.buffer,
      inputPointer,
      input.byteLength,
    ).set(input);
    const status = exports.ugoite_protocol_invoke(
      inputPointer,
      input.byteLength,
    );
    if (status !== 0) {
      throw new Error(`Ugoite WASM protocol invocation failed: ${status}`);
    }

    const outputPointer = exports.ugoite_protocol_result_pointer();
    const outputLength = exports.ugoite_protocol_result_length();
    const output = textDecoder.decode(
      new Uint8Array(
        exports.memory.buffer,
        outputPointer,
        outputLength,
      ).slice(),
    );
    const parsed = parseJsonWithNumberSources<ProtocolEnvelope<T, TCode>>(
      output,
    );
    const envelope = parsed.value;
    if (!envelope.ok) {
      throw new UgoiteApiError<TCode>(envelope.error);
    }
    decodeTypedIntegerResponse(command, envelope.value, parsed.integerSource);
    return envelope.value;
  } finally {
    exports.ugoite_dealloc(inputPointer, input.byteLength);
    exports.ugoite_protocol_clear_result();
  }
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value) && typeof value === "object" && !Array.isArray(value);

/** Keep exact integers confined to explicitly typed Composition protocol paths. */
const encodeTypedIntegerCommand = (
  command: ProtocolCommand,
): ProtocolCommand => {
  if (command.action === "domain.canonicalize_composition_document") {
    // This command shape is constructed only by
    // canonicalizeCompositionDocument below; malformed documents remain the
    // user's payload and are still sent to Rust for validation.
    const value = command.value as Record<string, unknown>;
    return {
      ...command,
      value: {
        ...value,
        document: encodeCompositionDocumentIntegers(value.document),
      },
    };
  }
  if (command.action === "domain.compile_variable_graph.v1") {
    return { ...command, value: encodeVariableGraphIntegers(command.value) };
  }
  if (command.action === "prepare" && isRecord(command.body)) {
    const body = command.body;
    if (isRecord(body.variables)) {
      const variables = encodeMarkedIntegerVariableValues(body.variables);
      if (variables !== body.variables) {
        return { ...command, body: { ...body, variables } };
      }
    }
  }
  return command;
};

const decodeTypedIntegerResponse = (
  command: ProtocolCommand,
  value: unknown,
  source: (parent: object, key: string) => string | undefined,
): void => {
  if (command.action === "domain.canonicalize_composition_document") {
    const document = (value as { document?: unknown } | null | undefined)
      ?.document;
    decodeCompositionDocumentIntegers(document, source);
    return;
  }
  if (command.action === "domain.parse_variable_graph_input") {
    // The private command wrapper always supplies this request record. Keep
    // validation of its user-provided value_type and the Rust response below.
    const request = command.value as { value_type?: unknown };
    if (
      request?.value_type && isRecord(request.value_type) &&
      request.value_type.scalar_type === "integer" && isRecord(value) &&
      value.status === "valid"
    ) {
      value.value = exactIntegerSourceValue(value, "value", source);
    }
    return;
  }
  if (command.action !== "decode" || !isRecord(value)) return;
  if (command.operation === "entry.get") {
    decodeEntryFieldIntegers(value, source);
  } else if (command.operation === "composition.evaluate") {
    decodeCompositionEvaluationIntegers(value, source);
  } else if (command.operation === "composition.lint") {
    if (isRecord(value.value)) {
      decodeCompositionDocumentIntegers(value.value.document, source);
    }
  } else if (command.operation === "composition.preview") {
    decodeCompositionPreviewIntegerDefaults(value, source);
  }
};

/** Preserve unsafe integer tokens in entry scalar fields and scalar lists. */
const decodeEntryFieldIntegers = (
  entry: Record<string, unknown>,
  source: (parent: object, key: string) => string | undefined,
): void => {
  if (!isRecord(entry.fields)) return;
  const decode = (parent: Record<string, unknown>, key: string): void => {
    if (typeof parent[key] !== "number") return;
    const token = source(parent, key);
    if (!token || !/^-?(?:0|[1-9]\d*)$/.test(token)) return;
    const number = parent[key] as number;
    if (Number.isSafeInteger(number)) return;
    parent[key] = exactIntegerSourceValue(parent, key, source);
  };
  for (const [key, field] of Object.entries(entry.fields)) {
    if (Array.isArray(field)) {
      field.forEach((_item, index) =>
        decode(field as unknown as Record<string, unknown>, String(index))
      );
    } else {
      // Scalars decode here; objects and structured values remain opaque.
      decode(entry.fields, key);
    }
  }
};

const invokeProtocol = async <T, TCode extends string = string>(
  command: ProtocolCommand,
): Promise<T> =>
  invokeProtocolWithExports<T, TCode>(await loadExports(), command);

export type KonaseProtocolAction =
  | "konase.version"
  | "konase.new"
  | "konase.step"
  | "konase.context"
  | "konase.preview_write"
  | "konase.validate_mutation_receipt"
  | "konase.resolve_effect";

/** Invoke the portable Konase control-plane protocol in Rust/WASM. */
export const invokeKonase = async <T = unknown>(
  action: KonaseProtocolAction,
  value?: unknown,
): Promise<T> =>
  await invokeProtocol<T>(
    value === undefined ? { action } : { action, value },
  );

export const getWasmSupportedOperations = async (): Promise<
  UgoiteApiOperation[]
> => await invokeProtocol<UgoiteApiOperation[]>({ action: "operations" });

/** Validate an AssetReference with the canonical Rust domain implementation. */
export const validateAssetReference = async (
  value: unknown,
): Promise<AssetReference> =>
  await invokeProtocol<AssetReference>({
    action: "domain.validate_asset_reference",
    value,
  });

export type CompositionDocumentCanonicalization = {
  document: unknown;
  canonical_yaml: string;
  fingerprint: string;
};

export type CompositionTextTemplateSegment =
  | { kind: "literal"; text: string }
  | { kind: "variable"; variable: string };

export type VariableGraphScalarType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "decimal"
  | "date"
  | "timestamp"
  | "json";

export type VariableGraphScalarDescriptor = {
  scalar_type: VariableGraphScalarType;
  nullable: boolean;
};

export type VariableGraphLiteral =
  | { kind: "string" | "decimal" | "date" | "timestamp"; value: string }
  | { kind: "boolean"; value: boolean }
  | { kind: "integer"; value: string }
  | { kind: "float"; value: number }
  | { kind: "json"; value: unknown }
  | { kind: "null" };

export type VariableGraphArgumentBinding = {
  name: string;
  variable: string;
};

export type VariableGraphQuery =
  & (
    | { kind: "inline_sql"; sql: string }
    | {
      kind: "saved_sql";
      entry_id?: string | null;
      revision_id?: string | null;
      column?: string | null;
    }
  )
  & {
    arguments?: VariableGraphArgumentBinding[];
    form_bindings?: { relation: string; form_id: string }[];
    schema_fingerprint?: string;
  };

export type VariableGraphResultType =
  | { kind: "scalar"; value_type: VariableGraphScalarDescriptor }
  | {
    kind: "table";
    columns: {
      name: string;
      value_type: VariableGraphScalarDescriptor;
    }[];
  };

export type VariableGraphNode =
  | {
    id: string;
    kind: "constant";
    value_type: VariableGraphScalarDescriptor;
    value: VariableGraphLiteral;
  }
  | {
    id: string;
    kind: "input";
    value_type: VariableGraphScalarDescriptor;
    required: boolean;
    default?: VariableGraphLiteral | null;
  }
  | {
    id: string;
    kind: "query";
    returns: VariableGraphResultType;
    query: VariableGraphQuery;
  };

/** Strict v1 graph payload accepted by the pure WASM Domain compiler. */
export type VariableGraph = {
  variables: VariableGraphNode[];
};

export type VariableGraphDiagnosticCode =
  | "invalid_variable_id"
  | "duplicate_variable_id"
  | "invalid_argument_name"
  | "duplicate_argument_name"
  | "invalid_constant_value"
  | "invalid_input_default"
  | "invalid_result_descriptor"
  | "invalid_inline_sql"
  | "invalid_saved_sql_identity"
  | "invalid_saved_sql_column"
  | "missing_argument_binding"
  | "unused_argument_binding"
  | "missing_variable_reference"
  | "variable_type_mismatch"
  | "dependency_cycle";

export type VariableGraphDiagnostic = {
  code: VariableGraphDiagnosticCode;
  variable_id?: string;
  argument_name?: string;
  reference_id?: string;
  cycle?: string[];
};

export type CompiledVariableGraph = {
  contract_version: 1;
  topological_order: string[];
  dependencies: Record<string, string[]>;
};

/** Validate a strict typed graph and derive its deterministic dependency plan. */
export const compileVariableGraph = async (
  graph: VariableGraph,
): Promise<CompiledVariableGraph> =>
  await invokeProtocol<CompiledVariableGraph>({
    action: "domain.compile_variable_graph.v1",
    value: graph,
  });

/** Half-open UTF-8 byte offsets into the UTF-8 encoding of SqlDocument.source. */
export type SqlDocumentSourceRange = {
  start: number;
  end: number;
};

/** Compact tuple: original byte start/end and normalized replacement byte length. */
export type SqlTemplateReplacement = [number, number, number];

export type SqlTemplateNormalization = {
  sql: string;
  parameters: string[];
  replacements: SqlTemplateReplacement[];
  error?: {
    code: "missing_closing_braces" | "invalid_parameter_name";
    range: SqlDocumentSourceRange;
  };
};

export type SqlDocumentTokenKind =
  | "trivia"
  | "comment"
  | "word"
  | "quoted_identifier"
  | "literal"
  | "placeholder"
  | "punctuation"
  | "operator";

export type SqlDocumentToken = {
  kind: SqlDocumentTokenKind;
  range: SqlDocumentSourceRange;
};

export type SqlDocumentNodeKind =
  | "query_statement"
  | "other_statement"
  | "query"
  | "with_clause"
  | "common_table_expression"
  | "set_operation"
  | "set_operation_left"
  | "set_operation_right"
  | "select"
  | "select_list"
  | "select_item"
  | "from_clause"
  | "relation_tree"
  | "join"
  | "where_clause"
  | "group_by_clause"
  | "group_by_item"
  | "having_clause"
  | "order_by_clause"
  | "order_by_item"
  | "limit_clause"
  | "offset_clause"
  | "relation"
  | "table_factor"
  | "identifier_expression"
  | "compound_identifier_expression"
  | "literal_expression"
  | "binary_expression"
  | "unary_expression"
  | "function_expression"
  | "case_expression"
  | "cast_expression"
  | "subquery_expression"
  | "predicate_expression"
  | "nested_expression"
  | "other_expression"
  | "value";

export type SqlDocumentIdentifier = {
  value: string;
  quote_style: string | null;
};

export type SqlDocumentOrderDirection = "asc" | "desc";
export type SqlDocumentNullsOrder = "first" | "last";

export type SqlDocumentLiteralKind =
  | "number"
  | "string"
  | "boolean"
  | "null"
  | "placeholder"
  | "other";

/** Parser-derived syntax metadata; this is not an execution or type contract. */
export type SqlDocumentNodeData =
  | { kind: "projection_alias"; alias: SqlDocumentIdentifier }
  | { kind: "select"; distinct: boolean; distinct_on: boolean }
  | {
    kind: "group_by";
    mode: SqlDocumentGroupByMode;
    groups: number[][];
    editable: boolean;
  }
  | {
    kind: "wildcard";
    qualifier: SqlDocumentIdentifier[];
    qualifier_is_expression: boolean;
    has_options: boolean;
  }
  | {
    kind: "order_by_item";
    direction: SqlDocumentOrderDirection | null;
    nulls_order: SqlDocumentNullsOrder | null;
  }
  | { kind: "query_clause_value"; value: number | null }
  | { kind: "identifier"; value: string; quote_style: string | null }
  | { kind: "compound_identifier"; parts: SqlDocumentIdentifier[] }
  | {
    kind: "literal";
    literal_kind: SqlDocumentLiteralKind;
    raw: string | null;
  }
  | { kind: "typed_literal"; data_type: string; raw: string | null }
  | { kind: "array_literal" }
  | { kind: "binary_operator"; operator: string }
  | { kind: "unary_operator"; operator: string }
  | {
    kind: "function";
    name: SqlDocumentIdentifier[];
    argument_count: number | null;
    distinct: boolean;
    has_filter: boolean;
    has_over: boolean;
    has_argument_clauses: boolean;
  }
  | {
    kind: "case";
    has_operand: boolean;
    when_count: number;
    has_else: boolean;
  }
  | { kind: "cast"; cast_kind: string; target_type: string | null }
  | { kind: "predicate"; operator: string; negated: boolean }
  | { kind: "set_operation"; operator: string; quantifier: string }
  | { kind: "with_clause"; recursive: boolean; cte_count: number }
  | {
    kind: "common_table_expression";
    name: SqlDocumentIdentifier;
    columns: SqlDocumentIdentifier[];
    materialized: boolean | null;
  }
  | { kind: "relation"; name: SqlDocumentIdentifier[] }
  | {
    kind: "array_slice";
    one_based_start: string;
    one_based_end: string;
    start_range: SqlDocumentSourceRange;
    end_range: SqlDocumentSourceRange;
  }
  | {
    kind: "table_factor";
    factor_kind: string;
    alias: SqlDocumentIdentifier | null;
    column_aliases: SqlDocumentIdentifier[];
    has_column_alias_types: boolean;
  }
  | {
    kind: "unmodeled_expression";
    node_kind: SqlDocumentNodeKind;
    reason: string;
  };

export type SqlDocumentNode = {
  id: number;
  parent: number | null;
  kind: SqlDocumentNodeKind;
  range: SqlDocumentSourceRange | null;
  semantic: SqlDocumentNodeData | null;
};

export type SqlDocumentStatus =
  | "parsed"
  | "empty"
  | "invalid_sql"
  | "multiple_statements"
  | "select_required"
  | "too_complex"
  | "too_large";

export type SqlDocumentDiagnosticCode =
  | "sql_too_large"
  | "sql_too_complex"
  | "parse_failed"
  | "empty_document"
  | "single_statement_required"
  | "select_required";

export type SqlDocumentDiagnostic = {
  code: SqlDocumentDiagnosticCode;
  message: string;
  range: SqlDocumentSourceRange | null;
};

/** Parser view only; a parsed document has not passed executor admission. */
export type SqlDocument = {
  contract_version: number;
  source: string;
  status: SqlDocumentStatus;
  diagnostics: SqlDocumentDiagnostic[];
  tokens: SqlDocumentToken[];
  nodes: SqlDocumentNode[];
};

export type SqlDocumentEdit = {
  range: SqlDocumentSourceRange;
  expected_text: string;
  replacement: string;
};

export type SqlDocumentEditTransaction = {
  expected_source: string;
  edits: SqlDocumentEdit[];
};

export type SqlDocumentSemanticIdentifier = {
  value: string;
  quote_style: string | null;
};

export type SqlDocumentSemanticBinaryOperator =
  | "and"
  | "or"
  | "bitwise_and"
  | "bitwise_or"
  | "shift_left"
  | "shift_right"
  | "eq"
  | "not_eq"
  | "lt"
  | "lte"
  | "gt"
  | "gte"
  | "add"
  | "subtract"
  | "multiply"
  | "divide"
  | "modulo"
  | "concat";

export type SqlDocumentSetOperator = "union" | "intersect" | "except";
export type SqlDocumentSetQuantifier =
  | "default"
  | "all"
  | "distinct"
  | "by_name"
  | "all_by_name"
  | "distinct_by_name";
export type SqlDocumentAggregateFunction =
  | "count"
  | "sum"
  | "avg"
  | "min"
  | "max";
export type SqlDocumentGroupByMode =
  | "expressions"
  | "all"
  | "rollup"
  | "cube"
  | "grouping_sets";

export type SqlDocumentRelationFactor =
  | {
    kind: "table";
    name: SqlDocumentIdentifier[];
    alias: SqlDocumentIdentifier | null;
  }
  | {
    kind: "derived";
    query_node_id: number;
    alias: SqlDocumentIdentifier;
  }
  | { kind: "existing_table_factor"; node_id: number };

export type SqlDocumentJoinOperator =
  | "inner"
  | "left"
  | "left_outer"
  | "right"
  | "right_outer"
  | "full"
  | "full_outer"
  | "cross"
  | "natural"
  | "natural_inner"
  | "natural_left"
  | "natural_left_outer"
  | "natural_right"
  | "natural_right_outer"
  | "natural_full"
  | "natural_full_outer"
  | "left_semi"
  | "right_semi"
  | "left_anti"
  | "right_anti";

export type SqlDocumentJoinConstraint =
  | { kind: "none" }
  | {
    kind: "on";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | { kind: "using"; columns: SqlDocumentIdentifier[] };

export type SqlDocumentJoin = {
  operator: SqlDocumentJoinOperator;
  relation: SqlDocumentRelationFactor;
  constraint: SqlDocumentJoinConstraint;
};

export type SqlDocumentSemanticEdit =
  | { kind: "initialize_select"; relation: SqlDocumentIdentifier | null }
  | {
    kind: "replace_table_factor";
    node_id: number;
    expected_kind: "table_factor";
    factor: SqlDocumentRelationFactor;
  }
  | {
    kind: "add_from_relation";
    node_id: number;
    expected_kind: "select";
    factor: SqlDocumentRelationFactor;
  };

export type SqlDocumentSemanticEditTransaction = {
  contract_version: number;
  expected_source: string;
  edits: SqlDocumentSemanticEdit[];
};

export type SqlDocumentEditResult = {
  document: SqlDocument;
  undo: SqlDocumentEditTransaction;
};

export type SqlDocumentEditErrorCode =
  | "invalid_request"
  | "invalid_old_parameter_name"
  | "invalid_new_parameter_name"
  | "stale_source"
  | "source_too_large"
  | "source_too_complex"
  | "too_many_edits"
  | "document_not_parsed"
  | "contract_version_mismatch"
  | "node_not_found"
  | "node_kind_mismatch"
  | "node_range_unavailable"
  | "invalid_semantic_replacement"
  | "duplicate_node_target"
  | "invalid_range"
  | "range_out_of_bounds"
  | "not_utf8_boundary"
  | "overlapping_edits"
  | "preimage_mismatch"
  | "output_too_large";

/** Parse one bounded SQL draft through the shared Rust/WASM domain parser. */
export const parseSqlDocument = async (sql: string): Promise<SqlDocument> =>
  await invokeProtocol<SqlDocument>({
    action: "domain.parse_sql_document",
    value: { sql },
  });

export type SqlLintProfile = "standard" | "composition";

export interface SqlLintDiagnostic {
  code: string;
  severity: "error";
  message: string;
  range?: SqlDocumentSourceRange;
}

export interface SqlLintResponse {
  contract_version: number;
  profile: SqlLintProfile;
  diagnostics: SqlLintDiagnostic[];
}

/** Lint SQL through Rust/WASM without Space access, planning, or execution. */
export const lintSql = async (
  sql: string,
  profile: SqlLintProfile = "standard",
): Promise<SqlLintResponse> =>
  await invokeProtocol<SqlLintResponse>({
    action: "domain.lint_sql",
    value: { sql, profile },
  });

/** Normalize SQL template markers through the Rust domain authority. */
export const normalizeSqlTemplate = async (
  sql: string,
): Promise<SqlTemplateNormalization> =>
  await invokeProtocol<SqlTemplateNormalization>({
    action: "domain.normalize_sql_template",
    value: { sql },
  });

/** Scope-aware quoted Form references from the shared Rust SQL parser. */
export const collectSqlFormNameReferences = async (
  sql: string,
): Promise<string[]> =>
  await invokeProtocol<string[]>({
    action: "domain.collect_quoted_form_name_references",
    value: { sql },
  });

/** All quoted single-part SQL table relations, including stable legacy names. */
export const collectSqlFormRelationNames = async (
  sql: string,
): Promise<string[]> =>
  await invokeProtocol<string[]>({
    action: "domain.collect_quoted_form_relation_names",
    value: { sql },
  });

/** Rename a SQL parameter through the shared Rust/WASM document contract.
 * Parameter names omit the $ prefix.
 */
export const renameSqlDocumentParameter = async (
  sql: string,
  oldName: string,
  newName: string,
): Promise<SqlDocument> =>
  await invokeProtocol<SqlDocument>({
    action: "domain.rename_sql_document_parameter",
    value: { sql, old_name: oldName, new_name: newName },
  });

/** Apply typed SQL syntax edits in Rust. */
export const applySqlDocumentSemanticEditTransaction = async (
  currentSource: string,
  transaction: SqlDocumentSemanticEditTransaction,
): Promise<SqlDocumentEditResult> =>
  await invokeProtocol<SqlDocumentEditResult, SqlDocumentEditErrorCode>({
    action: "domain.apply_sql_document_semantic_edit_transaction",
    value: { current_source: currentSource, transaction },
  });

/** Rust owns Form SQL construction; server planning still binds and authorizes it. */
export const compositionFormQuerySql = async (
  relation: string,
  columns: readonly { column: string; label?: string }[],
  aggregate?: { kind: "count" } | {
    kind: "sum" | "avg" | "min" | "max";
    column: string;
  },
): Promise<{ sql: string; projection_aliases: string[] }> =>
  await invokeProtocol({
    action: "domain.composition_form_query_sql",
    value: { relation, columns, aggregate: aggregate ?? null },
  });

export type VariableGraphInputValueByType = {
  string: string;
  boolean: boolean;
  integer: string;
  float: number;
  decimal: string;
  date: string;
  timestamp: string;
  json: unknown;
};

export type VariableGraphInputParseResult<
  T extends VariableGraphScalarType = VariableGraphScalarType,
> =
  | { status: "unset" }
  | { status: "valid"; value: VariableGraphInputValueByType[T] }
  | { status: "invalid" };

/** Load the canonical parser before rendering graph-backed Input controls. */
export const prepareVariableGraphInputParser = async (): Promise<
  void
> => {
  await loadExports();
};

/** Canonicalize a typed Composition document through the shared Rust domain. */
export const canonicalizeCompositionDocument = async (
  document: unknown,
): Promise<CompositionDocumentCanonicalization> =>
  await invokeProtocol<CompositionDocumentCanonicalization>({
    action: "domain.canonicalize_composition_document",
    value: { document },
  });

/** Parse a Text template with the shared Rust domain contract. */
export const parseCompositionTextTemplate = async (
  template: string,
): Promise<{ segments: CompositionTextTemplateSegment[] }> =>
  await invokeProtocol({
    action: "domain.parse_composition_text_template",
    value: { template },
  });

/** Parse raw variable text using the shared Rust variable-graph contract. */
export const parseVariableGraphInput = async <
  T extends VariableGraphScalarType,
>(
  valueType: VariableGraphScalarDescriptor & { scalar_type: T },
  raw: string,
): Promise<VariableGraphInputParseResult<T>> =>
  await invokeProtocol<VariableGraphInputParseResult<T>>({
    action: "domain.parse_variable_graph_input",
    value: { value_type: valueType, raw },
  });

/** Parse synchronously after prepareVariableGraphInputParser resolves. */
export const parseVariableGraphInputSync = <
  T extends VariableGraphScalarType,
>(
  valueType: VariableGraphScalarDescriptor & { scalar_type: T },
  raw: string,
): VariableGraphInputParseResult<T> => {
  if (!loadedExports) {
    throw new Error("Variable graph input parser is not prepared");
  }
  return invokeProtocolWithExports<VariableGraphInputParseResult<T>>(
    loadedExports,
    {
      action: "domain.parse_variable_graph_input",
      value: { value_type: valueType, raw },
    },
  );
};

/** Exact protocol envelope for one spreadsheet-CSV encode request. */
export const buildSpreadsheetCsvRequest = (
  rows: readonly (readonly string[])[],
): {
  action: "domain.encode_spreadsheet_csv";
  value: readonly (readonly string[])[];
} => ({
  action: "domain.encode_spreadsheet_csv",
  value: rows,
});

/** Exact byte size of one encode request on the Rust/WASM bridge. */
export const spreadsheetCsvRequestBytes = (
  rows: readonly (readonly string[])[],
): number =>
  textEncoder.encode(JSON.stringify(buildSpreadsheetCsvRequest(rows))).length;

/** Encode derived CSV output with the shared spreadsheet-safety rule. */
export const encodeSpreadsheetCsv = async (
  rows: readonly (readonly string[])[],
): Promise<string> =>
  await invokeProtocol<string>(buildSpreadsheetCsvRequest(rows));

/** Interpret a raw field value using the Core Entry coercion. */
export const coerceEntryFieldValue = (
  fieldType: string,
  itemType: string | undefined,
  value: unknown,
): Promise<unknown> =>
  invokeProtocol<unknown>({
    action: "entry.coerce_field_value",
    value: { field_type: fieldType, item_type: itemType, value },
  });

/** Validate a structured Entry draft with the shared Rust boundary (PR1/PR2). */
export const validateEntryDraft = async (
  form: unknown,
  draft: unknown,
): Promise<unknown> =>
  await invokeProtocol<unknown>({
    action: "entry.validate_draft",
    value: { form, draft },
  });

export class UgoiteApiError<TCode extends string = string> extends Error {
  readonly kind: string;
  readonly code?: TCode;
  readonly contract_version?: number;
  readonly diagnostics?: VariableGraphDiagnostic[];
  readonly operation?: string;
  readonly status?: number;
  readonly detail?: unknown;
  readonly payload?: unknown;
  readonly mutationOutcome: MutationOutcome | null;
  readonly mutationCode?: string;

  constructor(error: ProtocolErrorPayload<TCode>) {
    super(error.message);
    this.name = "UgoiteApiError";
    this.kind = error.kind;
    this.code = (error.code ?? readErrorCode(error.detail) ??
      readErrorCode(error.payload)) as TCode | undefined;
    this.contract_version = error.contract_version;
    this.diagnostics = error.diagnostics;
    this.operation = error.operation;
    this.status = error.status;
    this.detail = error.detail;
    this.payload = error.payload;
    this.mutationOutcome = classifyMutationOutcome(error);
    this.mutationCode = this.mutationOutcome &&
        this.mutationOutcome !== "confirmed"
      ? MUTATION_ERROR_CODES[this.mutationOutcome]
      : undefined;
  }
}

const readErrorCode = (value: unknown): string | undefined => {
  if (!value || typeof value !== "object") return undefined;
  const code = (value as { code?: unknown }).code;
  return typeof code === "string" ? code : undefined;
};

export const prepareApiRequest = async (
  operation: UgoiteApiOperation,
  argumentsValue: Record<string, unknown> = {},
  body?: unknown,
): Promise<PreparedRequest> =>
  await invokeProtocol<PreparedRequest>({
    action: "prepare",
    operation,
    arguments: argumentsValue,
    body,
  });

const decodeApiResponse = async <T>(
  operation: UgoiteApiOperation,
  response: Response,
): Promise<T> => {
  let body: string;
  try {
    body = await response.text();
  } catch (error) {
    throw new UgoiteApiError({
      kind: "transport",
      operation,
      status: response.status,
      message: error instanceof Error ? error.message : String(error),
      detail: { cause: error instanceof Error ? error.name : "unknown" },
    });
  }
  return await invokeProtocol<T>({
    action: "decode",
    operation,
    response: {
      status: response.status,
      status_text: response.statusText,
      headers: [...response.headers.entries()].map(([name, value]) => ({
        name,
        value,
      })),
      body,
    },
  });
};

export type ProtocolFetchOptions =
  & Omit<
    ApiFetchOptions,
    "method" | "body"
  >
  & {
    body?: BodyInit | null;
  };

const executeProtocolRequest = async (
  operation: UgoiteApiOperation,
  argumentsValue: Record<string, unknown>,
  body: unknown,
  options: ProtocolFetchOptions,
): Promise<Response> => {
  const prepared = await prepareApiRequest(operation, argumentsValue, body);
  if (options.signal?.aborted) {
    throw options.signal.reason ?? new DOMException(
      "The operation was aborted",
      "AbortError",
    );
  }
  const headers = new Headers();
  for (const header of prepared.headers) {
    headers.set(header.name, header.value);
  }
  const optionHeaders = new Headers(options.headers);
  optionHeaders.forEach((value, name) => headers.set(name, value));

  const requestBody = prepared.body_kind === "json"
    ? prepared.body
    : options.body;
  try {
    return await apiFetch(prepared.path, {
      ...options,
      method: prepared.method,
      headers,
      body: requestBody,
    });
  } catch (error) {
    if (
      options.signal?.aborted &&
      classifyMutationOutcome({ operation, kind: "transport" }) === null
    ) {
      throw error;
    }
    // Once fetch has begun, a transport failure cannot establish whether the
    // server committed the write. Keep the operation attached for all shared
    // UI and machine-readable result handling.
    throw new UgoiteApiError({
      kind: "transport",
      operation,
      message: error instanceof Error ? error.message : String(error),
      detail: { cause: error instanceof Error ? error.name : "unknown" },
    });
  }
};

/**
 * Execute a named Ugoite operation.
 *
 * Rust/WASM owns HTTP method, encoded path/query, JSON serialization, and
 * response/error decoding. TypeScript owns the environment-specific fetch,
 * SSR auth forwarding, cookies, AbortSignal, and multipart objects.
 */
export const protocolFetch = async <T>(
  operation: UgoiteApiOperation,
  argumentsValue: Record<string, unknown> = {},
  body?: unknown,
  options: ProtocolFetchOptions = {},
): Promise<T> => {
  const response = await executeProtocolRequest(
    operation,
    argumentsValue,
    body,
    options,
  );
  return await decodeApiResponse<T>(operation, response);
};

/** Execute an operation whose successful response is not JSON (for example asset bytes). */
export const protocolFetchResponse = async (
  operation: UgoiteApiOperation,
  argumentsValue: Record<string, unknown> = {},
  options: ProtocolFetchOptions = {},
): Promise<Response> => {
  const response = await executeProtocolRequest(
    operation,
    argumentsValue,
    undefined,
    options,
  );
  if (!response.ok) {
    await decodeApiResponse<never>(operation, response);
  }
  return response;
};
