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
  "composition.snapshot_check",
  "composition.preview",
  "composition.evaluate",
  "composition.save",
  "composition.restore",
  "sql.list",
  "sql.get",
  "sql.create",
  "sql.update",
  "sql.delete",
  "sql.query",
  "sql.query.plan",
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
  if (command.operation === "composition.evaluate") {
    decodeCompositionEvaluationIntegers(value, source);
  } else if (command.operation === "composition.lint") {
    if (isRecord(value.value)) {
      decodeCompositionDocumentIntegers(value.value.document, source);
    }
  } else if (command.operation === "composition.preview") {
    decodeCompositionPreviewIntegerDefaults(value, source);
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

export type SqlTemplateReplacement = {
  source_range: SqlDocumentSourceRange;
  normalized_range: SqlDocumentSourceRange;
  replacement: string;
};

export type SqlTemplateNormalization = {
  sql: string;
  parameters: string[];
  replacements: SqlTemplateReplacement[];
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

export type SqlDocumentColumnReference = {
  qualifier?: SqlDocumentIdentifier | null;
  column: SqlDocumentIdentifier;
};

export type SqlDocumentOrderDirection = "asc" | "desc";
export type SqlDocumentNullsOrder = "first" | "last";
export type SqlDocumentFilterOperator =
  | "eq"
  | "not_eq"
  | "lt"
  | "lte"
  | "gt"
  | "gte"
  | "like"
  | "not_like"
  | "is_null"
  | "is_not_null"
  | "is_true"
  | "is_false"
  | "is_unknown"
  | "is_distinct_from"
  | "is_not_distinct_from"
  | "ilike"
  | "not_ilike"
  | "similar_to"
  | "not_similar_to";

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
    can_replace_argument: boolean;
    can_add_argument: boolean;
    can_remove_argument: boolean;
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
  /** Rust-derived editing projection. SQL source remains authoritative. */
  conditions: SqlDocumentConditionScope[];
};

export type SqlDocumentConditionGroupOperator = "and" | "or";
export type SqlDocumentConditionNode =
  | {
    kind: "group";
    node_id: number;
    operator: SqlDocumentConditionGroupOperator;
    children: SqlDocumentConditionNode[];
  }
  | {
    kind: "comparison";
    node_id: number;
    column_node_id: number;
    column: SqlDocumentColumnReference;
    operator: SqlDocumentFilterOperator;
    value: SqlDocumentSemanticLiteral | null;
  }
  | { kind: "advanced"; node_id: number; sql: string };
export type SqlDocumentConditionScope = {
  query_node_id: number;
  where_clause_node_id: number | null;
  expression: SqlDocumentConditionNode | null;
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

export type SqlDocumentNodeEdit = {
  node_id: number;
  expected_kind: SqlDocumentNodeKind;
  replacement: string;
};

export type SqlDocumentNodeEditTransaction = {
  contract_version: number;
  expected_source: string;
  edits: SqlDocumentNodeEdit[];
};

export type SqlDocumentSemanticIdentifier = {
  value: string;
  quote_style: string | null;
};

export type SqlDocumentSemanticLiteral =
  | { kind: "number"; value: string }
  | { kind: "string"; value: string }
  | { kind: "boolean"; value: boolean }
  | { kind: "null" }
  | { kind: "named_parameter"; name: string }
  | {
    kind: "typed_string";
    data_type: "date" | "timestamp" | "interval";
    value: string;
  }
  | { kind: "hex_string"; value: string };

export type SqlDocumentQueryLimit =
  | { kind: "literal"; literal: SqlDocumentSemanticLiteral }
  | {
    kind: "expression";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  };

export type SqlDocumentFilterCondition =
  | {
    kind: "group";
    operator: SqlDocumentConditionGroupOperator;
    children: SqlDocumentFilterCondition[];
  }
  | {
    kind: "comparison";
    column: SqlDocumentColumnReference;
    operator: SqlDocumentFilterOperator;
    value: SqlDocumentSemanticLiteral | null;
  }
  | { kind: "copy_node"; node_id: number; expected_kind: SqlDocumentNodeKind };

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
export type SqlDocumentSetOperand = "left" | "right";
export type SqlDocumentUnarySign = "positive" | "negative" | "not";
export type SqlDocumentSubqueryPredicateTemplate = "in" | "not_in";
export type SqlDocumentPredicateTemplate =
  | "in"
  | "not_in"
  | "between"
  | "not_between"
  | "like"
  | "not_like"
  | "is_null"
  | "is_not_null"
  | "is_true"
  | "is_false"
  | "is_unknown"
  | "is_distinct_from"
  | "is_not_distinct_from"
  | "ilike"
  | "not_ilike"
  | "similar_to"
  | "not_similar_to";
export type SqlDocumentCastType =
  | "varchar"
  | "integer"
  | "boolean"
  | "big_int"
  | "double"
  | "decimal"
  | "date"
  | "timestamp"
  | "json";
export type SqlDocumentCastTarget = {
  kind: SqlDocumentCastType;
  length: string | null;
  precision: string | null;
  scale: string | null;
};
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
  | {
    kind: "insert_filter_condition";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    condition: SqlDocumentFilterCondition;
  }
  | {
    kind: "update_filter_condition";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    condition: SqlDocumentFilterCondition;
  }
  | {
    kind: "remove_filter_condition";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "set_filter_group_operator";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    operator: SqlDocumentConditionGroupOperator;
  }
  | {
    kind: "set_projection_alias";
    node_id: number;
    expected_kind: "select_item";
    alias: SqlDocumentIdentifier | null;
  }
  | {
    kind: "add_filter_comparison";
    node_id: number;
    expected_kind: "select";
    column: SqlDocumentColumnReference;
    operator: SqlDocumentFilterOperator;
    literal: SqlDocumentSemanticLiteral | null;
  }
  | {
    kind: "add_order_by_column";
    node_id: number;
    expected_kind: "query";
    column: SqlDocumentColumnReference;
    direction: SqlDocumentOrderDirection;
    nulls_order: SqlDocumentNullsOrder | null;
  }
  | {
    kind: "initialize_select";
    relation: SqlDocumentIdentifier | null;
  }
  | {
    kind: "replace_identifier";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    parts: SqlDocumentSemanticIdentifier[];
  }
  | {
    kind: "replace_literal";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    literal: SqlDocumentSemanticLiteral;
  }
  | {
    kind: "replace_binary_operator";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    operator: SqlDocumentSemanticBinaryOperator;
  }
  | {
    kind: "replace_unary_sign";
    node_id: number;
    expected_kind: "unary_expression";
    sign: SqlDocumentUnarySign;
  }
  | {
    kind: "replace_set_operation";
    node_id: number;
    expected_kind: "set_operation";
    operator: SqlDocumentSetOperator;
    quantifier: SqlDocumentSetQuantifier;
  }
  | {
    kind: "add_set_operation_operand";
    node_id: number;
    expected_kind: "query";
    operator: SqlDocumentSetOperator;
    quantifier: SqlDocumentSetQuantifier;
  }
  | {
    kind: "remove_set_operation_operand";
    node_id: number;
    expected_kind: "set_operation";
    keep: SqlDocumentSetOperand;
  }
  | {
    kind: "swap_set_operation_operands";
    node_id: number;
    expected_kind: "set_operation";
  }
  | {
    kind: "replace_set_operation_operand";
    node_id: number;
    expected_kind: "set_operation_left" | "set_operation_right";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "replace_cast_target_type";
    node_id: number;
    expected_kind: "cast_expression";
    target_type: SqlDocumentCastType;
  }
  | {
    kind: "set_parameterized_cast_target";
    node_id: number;
    expected_kind: "cast_expression";
    target: SqlDocumentCastTarget;
  }
  | {
    kind: "wrap_expression_in_cast";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    target_type: SqlDocumentCastType;
  }
  | {
    kind: "wrap_expression_in_array";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "append_array_literal_element";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    literal: SqlDocumentSemanticLiteral;
  }
  | {
    kind: "remove_array_literal_element";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "move_array_literal_element";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    move_up: boolean;
  }
  | {
    kind: "wrap_expression_in_value_access";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    access:
      | { kind: "struct_field"; name: string }
      | { kind: "array_index"; one_based_index: string }
      | {
        kind: "array_slice";
        one_based_start: string;
        one_based_end: string;
      };
  }
  | {
    kind: "set_array_slice_bounds";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    one_based_start: string;
    one_based_end: string;
  }
  | {
    kind: "wrap_expression_in_aggregate";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    function: SqlDocumentAggregateFunction;
    distinct: boolean;
  }
  | {
    kind: "set_aggregate_function";
    node_id: number;
    expected_kind: "function_expression";
    function: SqlDocumentAggregateFunction;
  }
  | {
    kind: "set_aggregate_distinct";
    node_id: number;
    expected_kind: "function_expression";
    distinct: boolean;
  }
  | {
    kind: "set_aggregate_filter";
    node_id: number;
    expected_kind: "function_expression";
    enabled: boolean;
  }
  | {
    kind: "remove_aggregate";
    node_id: number;
    expected_kind: "function_expression";
  }
  | {
    kind: "wrap_expression_in_case";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "rename_common_table_expression";
    node_id: number;
    expected_kind: "common_table_expression";
    name: SqlDocumentIdentifier;
  }
  | {
    kind: "set_relation_column_aliases";
    node_id: number;
    expected_kind: "common_table_expression" | "table_factor";
    aliases: SqlDocumentIdentifier[];
  }
  | {
    kind: "add_common_table_expression";
    node_id: number;
    expected_kind: "query";
    name: SqlDocumentIdentifier;
  }
  | {
    kind: "remove_common_table_expression";
    node_id: number;
    expected_kind: "common_table_expression";
  }
  | {
    kind: "replace_common_table_expression_query";
    node_id: number;
    expected_kind: "common_table_expression";
    query_node_id: number;
  }
  | {
    kind: "move_common_table_expression";
    node_id: number;
    expected_kind: "common_table_expression";
    to_index: number;
  }
  | {
    kind: "wrap_expression_in_subquery_predicate";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    template: SqlDocumentSubqueryPredicateTemplate;
  }
  | {
    kind: "replace_expression_with_exists_subquery";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "replace_predicate_negation";
    node_id: number;
    expected_kind: "predicate_expression";
    negated: boolean;
  }
  | {
    kind: "add_case_branch";
    node_id: number;
    expected_kind: "case_expression";
  }
  | {
    kind: "remove_case_branch";
    node_id: number;
    expected_kind: "case_expression";
  }
  | {
    kind: "set_case_else";
    node_id: number;
    expected_kind: "case_expression";
    has_else: boolean;
  }
  | {
    kind: "set_case_operand";
    node_id: number;
    expected_kind: "case_expression";
    expression_node_id: number | null;
    expression_expected_kind: SqlDocumentNodeKind | null;
  }
  | {
    kind: "add_where_clause";
    node_id: number;
    expected_kind: "select";
  }
  | {
    kind: "set_filter_predicate";
    node_id: number;
    expected_kind: "where_clause" | "having_clause";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "remove_filter_clause";
    node_id: number;
    expected_kind: "where_clause" | "having_clause";
  }
  | {
    kind: "wrap_expression_in_predicate";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    template: SqlDocumentPredicateTemplate;
  }
  | {
    kind: "append_in_list_value";
    node_id: number;
    expected_kind: "predicate_expression";
  }
  | {
    kind: "remove_last_in_list_value";
    node_id: number;
    expected_kind: "predicate_expression";
  }
  | {
    kind: "compose_binary_expression";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    left_expression_node_id: number;
    left_expression_expected_kind: SqlDocumentNodeKind;
    operator: SqlDocumentSemanticBinaryOperator;
    right_expression_node_id: number;
    right_expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "replace_expression_from_node";
    node_id: number;
    expected_kind: SqlDocumentNodeKind;
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "set_projection_expression";
    node_id: number;
    expected_kind: "select_item";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "add_projection_expression";
    node_id: number;
    expected_kind: "select_list";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "add_projection_column";
    node_id: number;
    expected_kind: "select_list";
    column: SqlDocumentColumnReference;
  }
  | {
    kind: "expand_projection_wildcard";
    node_id: number;
    expected_kind: "select_item";
    columns: SqlDocumentColumnReference[];
  }
  | {
    kind: "add_scalar_subquery_projection";
    node_id: number;
    expected_kind: "select_list";
  }
  | {
    kind: "add_group_by_expression";
    node_id: number;
    expected_kind: "select";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "set_group_by_structure";
    node_id: number;
    expected_kind: "select";
    mode: SqlDocumentGroupByMode;
    groups: number[][];
  }
  | {
    kind: "remove_group_by_item";
    node_id: number;
    expected_kind: "group_by_item";
  }
  | {
    kind: "move_group_by_item";
    node_id: number;
    expected_kind: "group_by_item";
    to_index: number;
  }
  | {
    kind: "add_order_by_expression";
    node_id: number;
    expected_kind: "query";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
    direction: "asc" | "desc";
    nulls_order: "first" | "last" | null;
  }
  | {
    kind: "set_order_by_expression";
    node_id: number;
    expected_kind: "order_by_item";
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "set_order_by_options";
    node_id: number;
    expected_kind: "order_by_item";
    direction: SqlDocumentOrderDirection;
    nulls_order: SqlDocumentNullsOrder | null;
  }
  | {
    kind: "remove_order_by_item";
    node_id: number;
    expected_kind: "order_by_item";
  }
  | {
    kind: "move_order_by_item";
    node_id: number;
    expected_kind: "order_by_item";
    to_index: number;
  }
  | {
    kind: "set_select_distinct";
    node_id: number;
    expected_kind: "select";
    distinct: boolean;
  }
  | {
    kind: "set_limit_value";
    node_id: number;
    expected_kind: "query";
    value: number;
  }
  | {
    kind: "set_limit_expression";
    node_id: number;
    expected_kind: "query";
    value: SqlDocumentQueryLimit;
  }
  | {
    kind: "remove_limit";
    node_id: number;
    expected_kind: "query";
  }
  | {
    kind: "set_offset_value";
    node_id: number;
    expected_kind: "query";
    value: number;
  }
  | {
    kind: "remove_offset";
    node_id: number;
    expected_kind: "query";
  }
  | {
    kind: "add_having_clause";
    node_id: number;
    expected_kind: "select";
  }
  | {
    kind: "remove_having_clause";
    node_id: number;
    expected_kind: "having_clause";
  }
  | {
    kind: "remove_projection_item";
    node_id: number;
    expected_kind: "select_item";
  }
  | {
    kind: "move_projection_item";
    node_id: number;
    expected_kind: "select_item";
    to_index: number;
  }
  | {
    kind: "set_function_argument";
    node_id: number;
    expected_kind: "function_expression";
    argument_index: number;
    expression_node_id: number;
    expression_expected_kind: SqlDocumentNodeKind;
  }
  | {
    kind: "remove_last_function_argument";
    node_id: number;
    expected_kind: "function_expression";
  }
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
  }
  | {
    kind: "add_join";
    node_id: number;
    expected_kind: "relation_tree";
    join: SqlDocumentJoin;
  }
  | {
    kind: "replace_join";
    node_id: number;
    expected_kind: "join";
    join: SqlDocumentJoin;
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
  | "projection_position_conflict"
  | "projection_alias_conflict"
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

/** Apply one guarded, atomic source edit and return its reparsed draft and undo. */
export const applySqlDocumentEditTransaction = async (
  currentSource: string,
  transaction: SqlDocumentEditTransaction,
): Promise<SqlDocumentEditResult> =>
  await invokeProtocol<SqlDocumentEditResult, SqlDocumentEditErrorCode>({
    action: "domain.apply_sql_document_edit_transaction",
    value: { current_source: currentSource, transaction },
  });

/** Apply guarded replacements to Rust-classified SQL Document nodes. */
export const applySqlDocumentNodeEditTransaction = async (
  currentSource: string,
  transaction: SqlDocumentNodeEditTransaction,
): Promise<SqlDocumentEditResult> =>
  await invokeProtocol<SqlDocumentEditResult, SqlDocumentEditErrorCode>({
    action: "domain.apply_sql_document_node_edit_transaction",
    value: { current_source: currentSource, transaction },
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
  aggregate?: { kind: "count" } | {
    kind: "sum" | "avg" | "min" | "max";
    column: string;
  },
): Promise<{ sql: string }> =>
  await invokeProtocol({
    action: "domain.composition_form_query_sql",
    value: { relation, aggregate: aggregate ?? null },
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
