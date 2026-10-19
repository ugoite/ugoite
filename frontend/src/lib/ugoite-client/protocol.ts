import initializeWasm from "../generated/ugoite_wasm.wasm?init";
import { apiFetch, type ApiFetchOptions } from "../api";
import type { AssetReference } from "../types";
import {
  classifyMutationOutcome,
  MUTATION_ERROR_CODES,
  type MutationOutcome,
} from "../mutation-outcome";

export const UGOITE_API_OPERATIONS = [
  "auth.get_config",
  "auth.get_session",
  "auth.audit",
  "auth.clear_session",
  "auth.accept_invitation",
  "auth.list_sessions",
  "auth.revoke_session",
  "auth.step_up.start",
  "auth.step_up.status",
  "auth.step_up.approve",
  "auth.recovery.owner_start",
  "auth.recovery.owner_finish",
  "preferences.get",
  "preferences.patch",
  "space.list",
  "space.create",
  "space.get",
  "space.audit",
  "space.health",
  "change.list",
  "change.page",
  "change.query",
  "change.inspect",
  "change.affected.get",
  "change.revert.preview",
  "change.revert",
  "run.inspect",
  "run.undo.preview",
  "run.undo",
  "ugoite.apply",
  "pin.list",
  "pin.create",
  "pin.delete",
  "space.pin_diff",
  "space.patch",
  "space.test_connection",
  "space.members.list",
  "space.members.invite",
  "space.members.update_role",
  "space.members.revoke",
  "space.recovery.force_reset",
  "form.list_types",
  "form.list",
  "form.get",
  "form.upsert",
  "entry.list",
  "entry.query",
  "entry.query.count",
  "entry.get",
  "entry.create",
  "entry.update",
  "entry.delete",
  "entry.history",
  "entry.revision",
  "entry.restore",
  "composition.lint",
  "composition.list",
  "composition.get",
  "composition.history",
  "composition.resolve",
  "composition.preview",
  "composition.metric.query",
  "composition.save",
  "composition.restore",
  "sql.list",
  "sql.get",
  "sql.create",
  "sql.update",
  "sql.delete",
  "sql.query",
  "sql.query.count",
  "agent.list",
  "agent.create",
  "agent.revoke",
  "approval.issue",
  "access.get",
  "access.put",
  "asset.upload",
  "asset.read",
  "asset.delete",
  "asset.list",
] as const;

export type UgoiteApiOperation = (typeof UGOITE_API_OPERATIONS)[number];

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

type ProtocolErrorPayload = {
  kind: string;
  message: string;
  code?: string;
  contract_version?: number;
  diagnostics?: VariableGraphDiagnostic[];
  operation?: string;
  status?: number;
  detail?: unknown;
  payload?: unknown;
};

type ProtocolEnvelope<T> =
  | { ok: true; value: T }
  | { ok: false; error: ProtocolErrorPayload };

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

const invokeProtocolWithExports = <T>(
  exports: UgoiteWasmExports,
  command: unknown,
): T => {
  const input = textEncoder.encode(JSON.stringify(command));
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
    const envelope = JSON.parse(output) as ProtocolEnvelope<T>;
    if (!envelope.ok) {
      throw new UgoiteApiError(envelope.error);
    }
    return envelope.value;
  } finally {
    exports.ugoite_dealloc(inputPointer, input.byteLength);
    exports.ugoite_protocol_clear_result();
  }
};

const invokeProtocol = async <T>(command: unknown): Promise<T> =>
  invokeProtocolWithExports(await loadExports(), command);

export type KonaseProtocolAction =
  | "konase.version"
  | "konase.new"
  | "konase.step"
  | "konase.context";

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
  | { kind: "integer"; value: number }
  | { kind: "float"; value: number }
  | { kind: "json"; value: unknown }
  | { kind: "null" };

export type VariableGraphArgumentBinding = {
  name: string;
  value_type: VariableGraphScalarDescriptor;
  variable: string;
};

export type VariableGraphQueryDefinition =
  | { kind: "inline_sql"; sql: string }
  | {
    kind: "saved_sql";
    entry_id?: string | null;
    revision_id?: string | null;
    column?: string | null;
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

export type VariableGraphNodeKind =
  | {
    kind: "constant";
    value_type: VariableGraphScalarDescriptor;
    value: VariableGraphLiteral;
  }
  | {
    kind: "input";
    value_type: VariableGraphScalarDescriptor;
    required: boolean;
    default?: VariableGraphLiteral | null;
  }
  | {
    kind: "query";
    returns: VariableGraphResultType;
    query: VariableGraphQueryDefinition;
    arguments?: VariableGraphArgumentBinding[];
  };

export type VariableGraphNode = {
  id: string;
  kind: VariableGraphNodeKind;
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
  | "select"
  | "select_item"
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

export type SqlDocumentNode = {
  id: number;
  parent: number | null;
  kind: SqlDocumentNodeKind;
  range: SqlDocumentSourceRange | null;
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

/** Parse one bounded SQL draft through the shared Rust/WASM domain parser. */
export const parseSqlDocument = async (sql: string): Promise<SqlDocument> =>
  await invokeProtocol<SqlDocument>({
    action: "domain.parse_sql_document",
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

export type SqlStructureColumnType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "date"
  | "timestamp"
  | "decimal"
  | "json"
  | "other";

export type SqlStructureVariableType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "date"
  | "timestamp";

export type SqlStructureContext = {
  relation: string;
  columns: { name: string; type: SqlStructureColumnType }[];
  variables?: { name: string; type: SqlStructureVariableType }[];
  stable_key?: string[];
};

export type SqlStructureFilterOperator =
  | "eq"
  | "not_eq"
  | "lt"
  | "lt_eq"
  | "gt"
  | "gt_eq"
  | "is_null"
  | "is_not_null";

export type SqlStructureFilter = {
  column: string;
  operator: SqlStructureFilterOperator;
  parameter?: string;
};

export type SqlStructureSort = {
  column: string;
  direction: "asc" | "desc";
};

export type SqlStructureAggregateFunction =
  | "count"
  | "sum"
  | "avg"
  | "min"
  | "max";

export type SqlStructureAlias = {
  name: string;
  quoted: boolean;
};

export type SqlStructureAggregate = {
  function: SqlStructureAggregateFunction;
  /** Omitted only for COUNT(*). */
  column?: string;
  distinct: boolean;
  alias?: SqlStructureAlias;
};

export type SqlStructureOperation =
  | { kind: "set_columns"; columns: string[] }
  | { kind: "set_filters"; filters: SqlStructureFilter[] }
  | { kind: "set_sort"; order_by: SqlStructureSort[] }
  | { kind: "set_aggregates"; aggregates: SqlStructureAggregate[] }
  | { kind: "set_group_by"; columns: string[] }
  | { kind: "set_distinct"; distinct: boolean };

export type SqlStructureReason =
  | "invalid_context"
  | "sql_too_large"
  | "parse_failed"
  | "single_statement_required"
  | "select_required"
  | "common_table_expression_unsupported"
  | "set_operation_unsupported"
  | "query_clause_unsupported"
  | "relation_unsupported"
  | "relation_mismatch"
  | "join_unsupported"
  | "projection_unsupported"
  | "filter_unsupported"
  | "sort_unsupported"
  | "unknown_column"
  | "duplicate_column"
  | "empty_projection"
  | "invalid_variable_name"
  | "unknown_variable"
  | "variable_type_mismatch"
  | "filter_type_unsupported"
  | "stable_key_unavailable"
  | "stable_key_unorderable"
  | "stable_order_required"
  | "aggregate_unsupported"
  | "aggregate_type_unsupported"
  | "group_by_unsupported"
  | "distinct_unsupported"
  | "round_trip_failed";

export type SqlStructureActionCapability = {
  enabled: boolean;
  reason?: SqlStructureReason;
};

export type SqlStructureAnalysis = {
  contract_version: number;
  status: "supported" | "invalid_sql" | "unsupported";
  reason?: SqlStructureReason;
  projection: string[];
  filters: SqlStructureFilter[];
  aggregates: SqlStructureAggregate[];
  group_by: string[];
  distinct: boolean;
  order_by: SqlStructureSort[];
  referenced_variables: string[];
  stable_pagination: boolean;
  capabilities: {
    columns: SqlStructureActionCapability;
    filter: SqlStructureActionCapability;
    sort: SqlStructureActionCapability;
    aggregate: SqlStructureActionCapability;
    group_by: SqlStructureActionCapability;
    distinct: SqlStructureActionCapability;
    stable_pagination: SqlStructureActionCapability;
  };
};

export type SqlStructureProposal = {
  contract_version: number;
  status: "supported" | "invalid_sql" | "unsupported";
  reason?: SqlStructureReason;
  base_sql: string;
  proposed_sql?: string;
  requires_confirmation: boolean;
  added_stable_tie_breakers: string[];
};

export type CompositionVariableInputType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "date"
  | "timestamp";

export type CompositionVariableInputValueByType = {
  string: string;
  boolean: boolean;
  integer: number;
  float: number;
  date: string;
  timestamp: string;
};

export type CompositionVariableInputParseResult<
  T extends CompositionVariableInputType = CompositionVariableInputType,
> =
  | { status: "unset" }
  | { status: "valid"; value: CompositionVariableInputValueByType[T] }
  | { status: "invalid" };

/** Load the canonical parser before rendering interactive Composition Inputs. */
export const prepareCompositionVariableInputParser = async (): Promise<
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

/** Parse raw Composition Input text using the shared Rust Domain contract. */
export const parseCompositionVariableInput = async <
  T extends CompositionVariableInputType,
>(
  type: T,
  raw: string,
): Promise<CompositionVariableInputParseResult<T>> =>
  await invokeProtocol<CompositionVariableInputParseResult<T>>({
    action: "domain.parse_composition_variable_input",
    value: { type, raw },
  });

/** Parse synchronously after prepareCompositionVariableInputParser resolves. */
export const parseCompositionVariableInputSync = <
  T extends CompositionVariableInputType,
>(
  type: T,
  raw: string,
): CompositionVariableInputParseResult<T> => {
  if (!loadedExports) {
    throw new Error("Composition variable parser is not prepared");
  }
  return invokeProtocolWithExports<CompositionVariableInputParseResult<T>>(
    loadedExports,
    {
      action: "domain.parse_composition_variable_input",
      value: { type, raw },
    },
  );
};

/** Load the Rust SQL parser before offering structure-aware Workspace actions. */
export const prepareSqlStructureParser = async (): Promise<void> => {
  await loadExports();
};

/** Analyze the safe SQL subset through the shared Rust domain parser. */
export const analyzeSqlStructure = async (
  sql: string,
  context: SqlStructureContext,
): Promise<SqlStructureAnalysis> =>
  await invokeProtocol<SqlStructureAnalysis>({
    action: "domain.analyze_sql_structure",
    value: { sql, context },
  });

/** Build a non-mutating SQL proposal; the adapter owns diff confirmation. */
export const proposeSqlStructureOperation = async (
  sql: string,
  context: SqlStructureContext,
  operation: SqlStructureOperation,
): Promise<SqlStructureProposal> =>
  await invokeProtocol<SqlStructureProposal>({
    action: "domain.propose_sql_structure_operation",
    value: { sql, context, operation },
  });

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

/** Validate a structured Entry draft with the shared Rust boundary (PR1/PR2). */
export const validateEntryDraft = async (
  form: unknown,
  draft: unknown,
): Promise<unknown> =>
  await invokeProtocol<unknown>({
    action: "entry.validate_draft",
    value: { form, draft },
  });

export class UgoiteApiError extends Error {
  readonly kind: string;
  readonly code?: string;
  readonly contract_version?: number;
  readonly diagnostics?: VariableGraphDiagnostic[];
  readonly operation?: string;
  readonly status?: number;
  readonly detail?: unknown;
  readonly payload?: unknown;
  readonly mutationOutcome: MutationOutcome | null;
  readonly mutationCode?: string;

  constructor(error: ProtocolErrorPayload) {
    super(error.message);
    this.name = "UgoiteApiError";
    this.kind = error.kind;
    this.code = error.code ?? readErrorCode(error.detail) ??
      readErrorCode(error.payload);
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
