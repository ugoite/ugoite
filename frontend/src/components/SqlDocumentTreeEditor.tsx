import {
  type PlannedSqlColumns,
  type PlanSqlColumns,
  usablePlannedColumnNames,
  wildcardProjectionQualifier,
} from "~/lib/sql-document-columns";
import type { JSX } from "solid-js";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
  untrack,
} from "solid-js";
import {
  applySqlDocumentSemanticEditTransaction,
  parseSqlDocument,
} from "~/lib/ugoite-client";
import { IconButton } from "~/components/IconButton";
import type {
  SqlDocument,
  SqlDocumentAggregateFunction,
  SqlDocumentCastType,
  SqlDocumentCastTarget,
  SqlDocumentColumnReference,
  SqlDocumentJoinConstraint,
  SqlDocumentJoinOperator,
  SqlDocumentNode,
  SqlDocumentPredicateTemplate,
  SqlDocumentRelationFactor,
  SqlDocumentSemanticBinaryOperator,
  SqlDocumentSemanticEdit,
  SqlDocumentSemanticIdentifier,
  SqlDocumentSemanticLiteral,
  SqlDocumentSetOperand,
  SqlDocumentSetOperator,
  SqlDocumentSetQuantifier,
  SqlDocumentSubqueryPredicateTemplate,
  SqlDocumentUnarySign,
} from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";

interface SqlDocumentTreeEditorProps {
  sql: () => string;
  scope?: () => string;
  onSqlChange: (sql: string) => void;
  disabled?: () => boolean;
  onPlanColumns?: PlanSqlColumns;
  relationOptions?: () => readonly {
    relation: string;
    label: string;
    columns?: readonly string[];
  }[];
  initialNodeKinds?: () => readonly SqlDocumentNode["kind"][];
}

type LiteralKind = "number" | "string" | "boolean" | "null" | "named_parameter";

type LiteralDraft = {
  kind: LiteralKind | "";
  value: string;
};

type CaseBranchAction = "add" | "remove" | "wrap" | "";
type InListAction = "append" | "remove_last" | "";
type CommonTableExpressionAction = "add" | "remove" | "replace" | "move" | "";
type SetOperationAddAction = "add" | "";
type SetOperationKeepAction = SqlDocumentSetOperand | "swap" | "";
type FunctionArgumentAction = "set" | "remove" | "";
type ProjectionItemAction = "move" | "remove" | "";
type BinaryExpressionAction = "compose" | "replace" | "";
type GroupByExpressionAction = "add" | "";
type OrderByExpressionAction = "add" | "";
type OrderDirection = "asc" | "desc";
type NullsOrder = "first" | "last" | "";
type ExpressionLeafAction = "identifier" | "literal" | "";
type RelationEditAction =
  | "replace"
  | "add_join"
  | "replace_join"
  | "add_from"
  | "";
type RelationChoice = "table" | "derived";
type ListItemAction = "remove" | "move" | "";
type QueryClauseAction =
  | "set_limit"
  | "remove_limit"
  | "set_offset"
  | "remove_offset"
  | "";
type JoinConstraintChoice = "none" | "on" | "using";

const joinOperators: { kind: SqlDocumentJoinOperator; label: string }[] = [
  { kind: "inner", label: "INNER JOIN" },
  { kind: "left", label: "LEFT JOIN" },
  { kind: "left_outer", label: "LEFT OUTER JOIN" },
  { kind: "right", label: "RIGHT JOIN" },
  { kind: "right_outer", label: "RIGHT OUTER JOIN" },
  { kind: "full", label: "FULL JOIN" },
  { kind: "full_outer", label: "FULL OUTER JOIN" },
  { kind: "cross", label: "CROSS JOIN" },
  { kind: "natural", label: "NATURAL JOIN" },
  { kind: "natural_inner", label: "NATURAL INNER JOIN" },
  { kind: "natural_left", label: "NATURAL LEFT JOIN" },
  { kind: "natural_left_outer", label: "NATURAL LEFT OUTER JOIN" },
  { kind: "natural_right", label: "NATURAL RIGHT JOIN" },
  { kind: "natural_right_outer", label: "NATURAL RIGHT OUTER JOIN" },
  { kind: "natural_full", label: "NATURAL FULL JOIN" },
  { kind: "natural_full_outer", label: "NATURAL FULL OUTER JOIN" },
];
const predicateTemplates: {
  kind: SqlDocumentPredicateTemplate;
  label: string;
}[] = [
  { kind: "in", label: "IN" },
  { kind: "not_in", label: "NOT IN" },
  { kind: "between", label: "BETWEEN" },
  { kind: "not_between", label: "NOT BETWEEN" },
  { kind: "like", label: "LIKE" },
  { kind: "not_like", label: "NOT LIKE" },
  { kind: "is_null", label: "IS NULL" },
  { kind: "is_not_null", label: "IS NOT NULL" },
];

const expressionNodeKinds = new Set<SqlDocumentNode["kind"]>([
  "identifier_expression",
  "compound_identifier_expression",
  "literal_expression",
  "binary_expression",
  "unary_expression",
  "function_expression",
  "case_expression",
  "cast_expression",
  "subquery_expression",
  "predicate_expression",
  "nested_expression",
  "other_expression",
  "value",
]);

const identifierQuoteStyles = [null, '"', "`", "["] as const;
const castTypes: { kind: SqlDocumentCastType; label: string }[] = [
  { kind: "varchar", label: "VARCHAR" },
  { kind: "integer", label: "INTEGER" },
  { kind: "boolean", label: "BOOLEAN" },
  { kind: "big_int", label: "BIGINT" },
  { kind: "double", label: "DOUBLE" },
  { kind: "decimal", label: "DECIMAL" },
  { kind: "date", label: "DATE" },
  { kind: "timestamp", label: "TIMESTAMP" },
];

const castTypeKind = (targetType: string | null): SqlDocumentCastType | "" =>
  castTypes.find((item) =>
    item.label === targetType?.trim().toUpperCase().replace(/\(.*/, "")
  )
    ?.kind ?? "";

const castTargetFrom = (
  kind: SqlDocumentCastType | "",
  raw: string | null,
): SqlDocumentCastTarget | undefined => {
  if (!kind) return undefined;
  const parameters = raw?.match(/\((\d+)(?:\s*,\s*(-?\d+))?\)/);
  const first = parameters ? parameters[1] : null;
  const second = parameters?.[2] === undefined ? null : parameters[2];
  return {
    kind,
    length: kind === "varchar" ? first : null,
    precision: kind === "decimal" || kind === "timestamp" ? first : null,
    scale: kind === "decimal" ? second : null,
  };
};

const aggregateFunctionKind = (
  node: SqlDocumentNode,
): SqlDocumentAggregateFunction | "" => {
  if (
    node.kind !== "function_expression" || node.semantic?.kind !== "function"
  ) {
    return "";
  }
  const name = node.semantic.name.at(-1)?.value.toLowerCase();
  return aggregateFunctions.find((item) => item.kind === name)?.kind ?? "";
};

const editableCastKinds = new Set([
  "cast",
  "try_cast",
  "safe_cast",
  "double_colon",
  "convert",
  "try_convert",
]);

const isEditableCastNode = (node: SqlDocumentNode): boolean =>
  node.kind === "cast_expression" && node.semantic?.kind === "cast" &&
  editableCastKinds.has(node.semantic.cast_kind);

const aggregateFunctions: {
  kind: SqlDocumentAggregateFunction;
  label: string;
}[] = [
  { kind: "count", label: "COUNT" },
  { kind: "sum", label: "SUM" },
  { kind: "avg", label: "AVG" },
  { kind: "min", label: "MIN" },
  { kind: "max", label: "MAX" },
];

const binaryOperators: {
  kind: SqlDocumentSemanticBinaryOperator;
  spelling: string[];
  label: string;
}[] = [
  { kind: "and", spelling: ["AND"], label: "AND" },
  { kind: "or", spelling: ["OR"], label: "OR" },
  { kind: "bitwise_and", spelling: ["&"], label: "&" },
  { kind: "bitwise_or", spelling: ["|"], label: "|" },
  { kind: "shift_left", spelling: ["<<"], label: "<<" },
  { kind: "shift_right", spelling: [">>"], label: ">>" },
  { kind: "eq", spelling: ["="], label: "=" },
  { kind: "not_eq", spelling: ["<>", "!="], label: "<>" },
  { kind: "lt", spelling: ["<"], label: "<" },
  { kind: "lte", spelling: ["<="], label: "<=" },
  { kind: "gt", spelling: [">"], label: ">" },
  { kind: "gte", spelling: [">="], label: ">=" },
  { kind: "add", spelling: ["+"], label: "+" },
  { kind: "subtract", spelling: ["-"], label: "−" },
  { kind: "multiply", spelling: ["*"], label: "×" },
  { kind: "divide", spelling: ["/"], label: "÷" },
  { kind: "modulo", spelling: ["%"], label: "%" },
  { kind: "concat", spelling: ["||"], label: "||" },
];
const negatablePredicates = new Set([
  "in",
  "in_subquery",
  "in_unnest",
  "between",
  "like",
  "ilike",
  "similar_to",
  "rlike",
  "is_null",
  "is_true",
  "is_false",
  "is_unknown",
  "is_distinct_from",
]);
const unarySignKind = (operator: string): SqlDocumentUnarySign | "" =>
  operator === "+" ? "positive" : operator === "-" ? "negative" :
  operator.toUpperCase() === "NOT" ? "not" : "";

const decodeSimpleString = (raw: string | null): string | undefined => {
  if (!raw || raw.length < 2) return undefined;
  const quote = raw[0];
  if ((quote !== "'" && quote !== '"') || raw.at(-1) !== quote) {
    return undefined;
  }
  const body = raw.slice(1, -1);
  return body.replaceAll(quote + quote, quote);
};

const initialLiteralDraft = (node: SqlDocumentNode): LiteralDraft => {
  if (node.semantic?.kind !== "literal") return { kind: "", value: "" };
  const raw = node.semantic.raw;
  switch (node.semantic.literal_kind) {
    case "number":
      return { kind: "number", value: raw ?? "" };
    case "string": {
      const value = decodeSimpleString(raw);
      return value === undefined
        ? { kind: "", value: "" }
        : { kind: "string", value };
    }
    case "boolean":
      return {
        kind: "boolean",
        value: raw?.toLowerCase() === "true" ? "true" : "false",
      };
    case "null":
      return { kind: "null", value: "" };
    case "placeholder":
      return { kind: "named_parameter", value: raw?.replace(/^\$/, "") ?? "" };
    case "other":
      return { kind: "", value: "" };
  }
};

const documentNodeLabel = (kind: SqlDocumentNode["kind"]): string => {
  const key = `sqlPage.structure.documentNode.${kind}`;
  return t(key);
};

function utf8OffsetToUtf16(
  source: string,
  byteOffset: number,
): number | undefined {
  if (!Number.isInteger(byteOffset) || byteOffset < 0) return undefined;
  let bytes = 0;
  let utf16 = 0;
  for (const character of source) {
    if (bytes === byteOffset) return utf16;
    const codePoint = character.codePointAt(0)!;
    const byteLength = codePoint <= 0x7f
      ? 1
      : codePoint <= 0x7ff
      ? 2
      : codePoint <= 0xffff
      ? 3
      : 4;
    if (bytes + byteLength > byteOffset) return undefined;
    bytes += byteLength;
    utf16 += character.length;
  }
  return bytes === byteOffset ? utf16 : undefined;
}

function sourceSlice(source: string, node: SqlDocumentNode): string {
  if (!node.range) return "";
  const start = utf8OffsetToUtf16(source, node.range.start);
  const end = utf8OffsetToUtf16(source, node.range.end);
  if (start === undefined || end === undefined || end < start) return "";
  return source.slice(start, end).replace(/\s+/g, " ").trim();
}

function operatorKind(
  operator: string,
): SqlDocumentSemanticBinaryOperator | undefined {
  return binaryOperators.find((item) =>
    item.spelling.includes(operator.toUpperCase())
  )?.kind;
}

function identifierParts(
  node: SqlDocumentNode,
): SqlDocumentSemanticIdentifier[] {
  if (node.semantic?.kind === "identifier") {
    return [{
      value: node.semantic.value,
      quote_style: node.semantic.quote_style,
    }];
  }
  if (node.semantic?.kind === "compound_identifier") {
    return node.semantic.parts.map((part) => ({ ...part }));
  }
  return [];
}

function commonTableExpressionName(
  node: SqlDocumentNode,
): SqlDocumentSemanticIdentifier {
  return node.semantic?.kind === "common_table_expression"
    ? { ...node.semantic.name }
    : { value: "", quote_style: null };
}

function semanticEdit(
  node: SqlDocumentNode,
  expressionLeafAction: ExpressionLeafAction,
  parts: readonly SqlDocumentSemanticIdentifier[],
  literal: LiteralDraft,
  operator: SqlDocumentSemanticBinaryOperator | "",
  setOperator: SqlDocumentSetOperator | "",
  setQuantifier: SqlDocumentSetQuantifier | "",
  setOperationAdd: SetOperationAddAction,
  setOperationKeep: SetOperationKeepAction,
  castTarget: SqlDocumentCastTarget | undefined,
  wrapCastType: SqlDocumentCastType | "",
  wrapAggregateFunction: SqlDocumentAggregateFunction | "",
  wrapAggregateDistinct: boolean,
  cteName: SqlDocumentSemanticIdentifier,
  cteAction: CommonTableExpressionAction,
  predicateIsNegated: boolean,
  unarySign: SqlDocumentUnarySign | "",
  caseBranchAction: CaseBranchAction,
  caseHasElse: boolean,
  caseOperandAction: "set" | "clear" | "",
  caseOperandNode: SqlDocumentNode | undefined,
  projectionExpressionNode: SqlDocumentNode | undefined,
  binaryExpressionAction: BinaryExpressionAction,
  binaryExpressionLeftNode: SqlDocumentNode | undefined,
  binaryExpressionOperator: SqlDocumentSemanticBinaryOperator | "",
  binaryExpressionRightNode: SqlDocumentNode | undefined,
  functionArgumentAction: FunctionArgumentAction,
  functionArgumentIndex: number,
  functionExpressionNode: SqlDocumentNode | undefined,
  groupByExpressionAction: GroupByExpressionAction,
  projectionItemAction: ProjectionItemAction = "",
  projectionMoveToIndex?: number,
  orderByExpressionAction: OrderByExpressionAction = "",
  orderDirection: OrderDirection = "asc",
  nullsOrder: NullsOrder = "",
  addWhere: boolean = false,
  predicateTemplate: SqlDocumentPredicateTemplate | "" = "",
  inListAction: InListAction = "",
  addScalarSubquery: boolean = false,
): SqlDocumentSemanticEdit | undefined {
  if (node.kind === "select_list" && addScalarSubquery) {
    return {
      kind: "add_scalar_subquery_projection",
      node_id: node.id,
      expected_kind: "select_list",
    };
  }
  if (
    node.kind === "predicate_expression" &&
    node.semantic?.kind === "predicate" &&
    node.semantic.operator === "in"
  ) {
    if (inListAction === "append") {
      return {
        kind: "append_in_list_value",
        node_id: node.id,
        expected_kind: "predicate_expression",
      };
    }
    if (inListAction === "remove_last") {
      return {
        kind: "remove_last_in_list_value",
        node_id: node.id,
        expected_kind: "predicate_expression",
      };
    }
  }
  if (predicateTemplate && expressionNodeKinds.has(node.kind)) {
    return {
      kind: "wrap_expression_in_predicate",
      node_id: node.id,
      expected_kind: node.kind,
      template: predicateTemplate,
    };
  }
  if (addWhere && node.kind === "select") {
    return {
      kind: "add_where_clause",
      node_id: node.id,
      expected_kind: "select",
    };
  }
  if (caseBranchAction === "wrap" && expressionNodeKinds.has(node.kind)) {
    return {
      kind: "wrap_expression_in_case",
      node_id: node.id,
      expected_kind: node.kind,
    };
  }
  if (wrapAggregateFunction && expressionNodeKinds.has(node.kind)) {
    return {
      kind: "wrap_expression_in_aggregate",
      node_id: node.id,
      expected_kind: node.kind,
      function: wrapAggregateFunction,
      distinct: wrapAggregateDistinct,
    };
  }
  if (wrapCastType && expressionNodeKinds.has(node.kind)) {
    return {
      kind: "wrap_expression_in_cast",
      node_id: node.id,
      expected_kind: node.kind,
      target_type: wrapCastType,
    };
  }
  if (
    expressionLeafAction === "identifier" && expressionNodeKinds.has(node.kind)
  ) {
    return {
      kind: "replace_identifier",
      node_id: node.id,
      expected_kind: node.kind,
      parts: [...parts],
    };
  }
  if (
    expressionLeafAction === "literal" && expressionNodeKinds.has(node.kind)
  ) {
    let value: SqlDocumentSemanticLiteral | undefined;
    switch (literal.kind) {
      case "number":
        value = { kind: "number", value: literal.value };
        break;
      case "string":
        value = { kind: "string", value: literal.value };
        break;
      case "boolean":
        value = { kind: "boolean", value: literal.value === "true" };
        break;
      case "null":
        value = { kind: "null" };
        break;
      case "named_parameter":
        value = { kind: "named_parameter", name: literal.value };
        break;
    }
    if (value) {
      return {
        kind: "replace_literal",
        node_id: node.id,
        expected_kind: node.kind,
        literal: value,
      };
    }
  }
  if (
    node.kind === "query" && orderByExpressionAction === "add" &&
    projectionExpressionNode
  ) {
    return {
      kind: "add_order_by_expression",
      node_id: node.id,
      expected_kind: "query",
      expression_node_id: projectionExpressionNode.id,
      expression_expected_kind: projectionExpressionNode.kind,
      direction: orderDirection,
      nulls_order: nullsOrder || null,
    };
  }
  if (
    node.kind === "select" && groupByExpressionAction === "add" &&
    projectionExpressionNode
  ) {
    return {
      kind: "add_group_by_expression",
      node_id: node.id,
      expected_kind: "select",
      expression_node_id: projectionExpressionNode.id,
      expression_expected_kind: projectionExpressionNode.kind,
    };
  }
  if (
    binaryExpressionAction === "replace" && projectionExpressionNode &&
    expressionNodeKinds.has(node.kind)
  ) {
    return {
      kind: "replace_expression_from_node",
      node_id: node.id,
      expected_kind: node.kind,
      expression_node_id: projectionExpressionNode.id,
      expression_expected_kind: projectionExpressionNode.kind,
    };
  }
  if (
    (node.kind === "select_item" || expressionNodeKinds.has(node.kind)) &&
    binaryExpressionAction === "compose" &&
    binaryExpressionLeftNode && binaryExpressionOperator &&
    binaryExpressionRightNode
  ) {
    return {
      kind: "compose_binary_expression",
      node_id: node.id,
      expected_kind: node.kind,
      left_expression_node_id: binaryExpressionLeftNode.id,
      left_expression_expected_kind: binaryExpressionLeftNode.kind,
      operator: binaryExpressionOperator,
      right_expression_node_id: binaryExpressionRightNode.id,
      right_expression_expected_kind: binaryExpressionRightNode.kind,
    };
  }
  if (
    node.kind === "identifier_expression" ||
    node.kind === "compound_identifier_expression"
  ) {
    return {
      kind: "replace_identifier",
      node_id: node.id,
      expected_kind: node.kind,
      parts: [...parts],
    };
  }
  if (node.kind === "value" || node.kind === "literal_expression") {
    let value: SqlDocumentSemanticLiteral | undefined;
    switch (literal.kind) {
      case "number":
        value = { kind: "number", value: literal.value };
        break;
      case "string":
        value = { kind: "string", value: literal.value };
        break;
      case "boolean":
        value = { kind: "boolean", value: literal.value === "true" };
        break;
      case "null":
        value = { kind: "null" };
        break;
      case "named_parameter":
        value = { kind: "named_parameter", name: literal.value };
        break;
    }
    if (value) {
      return {
        kind: "replace_literal",
        node_id: node.id,
        expected_kind: node.kind,
        literal: value,
      };
    }
  }
  if (node.kind === "binary_expression" && operator) {
    return {
      kind: "replace_binary_operator",
      node_id: node.id,
      expected_kind: node.kind,
      operator,
    };
  }
  if (
    node.kind === "unary_expression" &&
    node.semantic?.kind === "unary_operator" &&
    ["+", "-", "NOT"].includes(node.semantic.operator.toUpperCase()) && unarySign
  ) {
    return {
      kind: "replace_unary_sign",
      node_id: node.id,
      expected_kind: "unary_expression",
      sign: unarySign,
    };
  }
  if (node.kind === "set_operation" && setOperationKeep) {
    if (setOperationKeep === "swap") {
      return {
        kind: "swap_set_operation_operands",
        node_id: node.id,
        expected_kind: "set_operation",
      };
    }
    return {
      kind: "remove_set_operation_operand",
      node_id: node.id,
      expected_kind: "set_operation",
      keep: setOperationKeep,
    };
  }
  if (node.kind === "set_operation" && setOperator && setQuantifier) {
    return {
      kind: "replace_set_operation",
      node_id: node.id,
      expected_kind: "set_operation",
      operator: setOperator,
      quantifier: setQuantifier,
    };
  }
  if (
    node.kind === "query" && setOperationAdd && setOperator && setQuantifier
  ) {
    return {
      kind: "add_set_operation_operand",
      node_id: node.id,
      expected_kind: "query",
      operator: setOperator,
      quantifier: setQuantifier,
    };
  }
  if (isEditableCastNode(node) && castTarget) {
    return {
      kind: "set_parameterized_cast_target",
      node_id: node.id,
      expected_kind: "cast_expression",
      target: castTarget,
    };
  }
  if (
    node.kind === "query" && cteAction === "add" &&
    cteName.value.trim().length > 0
  ) {
    return {
      kind: "add_common_table_expression",
      node_id: node.id,
      expected_kind: "query",
      name: { ...cteName },
    };
  }
  if (node.kind === "common_table_expression" && cteAction === "remove") {
    return {
      kind: "remove_common_table_expression",
      node_id: node.id,
      expected_kind: "common_table_expression",
    };
  }
  if (
    node.kind === "common_table_expression" && cteName.value.trim().length > 0
  ) {
    return {
      kind: "rename_common_table_expression",
      node_id: node.id,
      expected_kind: "common_table_expression",
      name: { ...cteName },
    };
  }
  if (
    node.kind === "predicate_expression" &&
    node.semantic?.kind === "predicate" &&
    negatablePredicates.has(node.semantic.operator) &&
    predicateIsNegated !== node.semantic.negated
  ) {
    return {
      kind: "replace_predicate_negation",
      node_id: node.id,
      expected_kind: "predicate_expression",
      negated: predicateIsNegated,
    };
  }
  if (node.kind === "case_expression" && node.semantic?.kind === "case") {
    if (caseOperandAction === "set" && caseOperandNode) {
      return {
        kind: "set_case_operand",
        node_id: node.id,
        expected_kind: "case_expression",
        expression_node_id: caseOperandNode.id,
        expression_expected_kind: caseOperandNode.kind,
      };
    }
    if (caseOperandAction === "clear") {
      return {
        kind: "set_case_operand",
        node_id: node.id,
        expected_kind: "case_expression",
        expression_node_id: null,
        expression_expected_kind: null,
      };
    }
    if (caseBranchAction === "add") {
      return {
        kind: "add_case_branch",
        node_id: node.id,
        expected_kind: "case_expression",
      };
    }
    if (caseBranchAction === "remove") {
      return {
        kind: "remove_case_branch",
        node_id: node.id,
        expected_kind: "case_expression",
      };
    }
    if (caseHasElse !== node.semantic.has_else) {
      return {
        kind: "set_case_else",
        node_id: node.id,
        expected_kind: "case_expression",
        has_else: caseHasElse,
      };
    }
  }
  if (
    node.kind === "function_expression" &&
    node.semantic?.kind === "function"
  ) {
    if (functionArgumentAction === "remove") {
      return {
        kind: "remove_last_function_argument",
        node_id: node.id,
        expected_kind: "function_expression",
      };
    }
    if (functionArgumentAction === "set" && functionExpressionNode) {
      return {
        kind: "set_function_argument",
        node_id: node.id,
        expected_kind: "function_expression",
        argument_index: functionArgumentIndex,
        expression_node_id: functionExpressionNode.id,
        expression_expected_kind: functionExpressionNode.kind,
      };
    }
  }
  if (node.kind === "select_list" && projectionExpressionNode) {
    return {
      kind: "add_projection_expression",
      node_id: node.id,
      expected_kind: "select_list",
      expression_node_id: projectionExpressionNode.id,
      expression_expected_kind: projectionExpressionNode.kind,
    };
  }
  if (node.kind === "select_item" && projectionItemAction === "remove") {
    return {
      kind: "remove_projection_item",
      node_id: node.id,
      expected_kind: "select_item",
    };
  }
  if (
    node.kind === "select_item" && projectionItemAction === "move" &&
    projectionMoveToIndex !== undefined
  ) {
    return {
      kind: "move_projection_item",
      node_id: node.id,
      expected_kind: "select_item",
      to_index: projectionMoveToIndex,
    };
  }
  if (node.kind === "select_item" && projectionExpressionNode) {
    return {
      kind: "set_projection_expression",
      node_id: node.id,
      expected_kind: "select_item",
      expression_node_id: projectionExpressionNode.id,
      expression_expected_kind: projectionExpressionNode.kind,
    };
  }
  return undefined;
}

function setOperatorKind(operator: string): SqlDocumentSetOperator | "" {
  switch (operator.toUpperCase()) {
    case "UNION":
      return "union";
    case "INTERSECT":
      return "intersect";
    case "EXCEPT":
      return "except";
    default:
      return "";
  }
}

function setQuantifierKind(
  quantifier: string,
): SqlDocumentSetQuantifier | "" {
  switch (quantifier.toUpperCase()) {
    case "":
      return "default";
    case "ALL":
      return "all";
    case "DISTINCT":
      return "distinct";
    case "BY NAME":
      return "by_name";
    case "ALL BY NAME":
      return "all_by_name";
    case "DISTINCT BY NAME":
      return "distinct_by_name";
    default:
      return "";
  }
}

export function SqlDocumentTreeEditor(props: SqlDocumentTreeEditorProps) {
  const [document, setDocument] = createSignal<SqlDocument>();
  const [selectedNodeId, setSelectedNodeId] = createSignal<number>();
  const [parts, setParts] = createSignal<SqlDocumentSemanticIdentifier[]>([]);
  const [literal, setLiteral] = createSignal<LiteralDraft>({
    kind: "",
    value: "",
  });
  const [expressionLeafAction, setExpressionLeafAction] = createSignal<
    ExpressionLeafAction
  >("");
  const [operator, setOperator] = createSignal<
    SqlDocumentSemanticBinaryOperator | ""
  >("");
  const [setOperationOperator, setSetOperationOperator] = createSignal<
    SqlDocumentSetOperator | ""
  >("");
  const [setOperationQuantifier, setSetOperationQuantifier] = createSignal<
    SqlDocumentSetQuantifier | ""
  >("default");
  const [setOperationAdd, setSetOperationAdd] = createSignal<
    SetOperationAddAction
  >("");
  const [setOperationKeep, setSetOperationKeep] = createSignal<
    SetOperationKeepAction
  >("");
  const [setOperationReplacementNodeId, setSetOperationReplacementNodeId] =
    createSignal<number>();
  const [castTargetType, setCastTargetType] = createSignal<
    SqlDocumentCastType | ""
  >("");
  const [castTargetDraft, setCastTargetDraft] = createSignal<
    SqlDocumentCastTarget | undefined
  >();
  const [wrapCastTargetType, setWrapCastTargetType] = createSignal<
    SqlDocumentCastType | ""
  >("");
  const [wrapAggregateFunction, setWrapAggregateFunction] = createSignal<
    SqlDocumentAggregateFunction | ""
  >("");
  const [wrapAggregateDistinct, setWrapAggregateDistinct] = createSignal(false);
  const [cteName, setCteName] = createSignal<SqlDocumentSemanticIdentifier>({
    value: "",
    quote_style: null,
  });
  const [cteAction, setCteAction] = createSignal<CommonTableExpressionAction>(
    "",
  );
  const [cteQueryNodeId, setCteQueryNodeId] = createSignal<number>();
  const [cteMoveToIndex, setCteMoveToIndex] = createSignal<number>();
  const [predicateIsNegated, setPredicateIsNegated] = createSignal(false);
  const [unarySign, setUnarySign] = createSignal<SqlDocumentUnarySign | "">(
    "",
  );
  const [caseBranchAction, setCaseBranchAction] = createSignal<
    CaseBranchAction
  >(
    "",
  );
  const [caseHasElse, setCaseHasElse] = createSignal(false);
  const [caseOperandAction, setCaseOperandAction] = createSignal<"set" | "clear" | "">("");
  const [caseOperandNodeId, setCaseOperandNodeId] = createSignal<number>();
  const [addWhereClause, setAddWhereClause] = createSignal(false);
  const [addScalarSubquery, setAddScalarSubquery] = createSignal(false);
  const [predicateTemplate, setPredicateTemplate] = createSignal<
    SqlDocumentPredicateTemplate | ""
  >("");
  const [subqueryPredicateTemplate, setSubqueryPredicateTemplate] =
    createSignal<SqlDocumentSubqueryPredicateTemplate | "">("");
  const [replaceWithExists, setReplaceWithExists] = createSignal(false);
  const [inListAction, setInListAction] = createSignal<InListAction>("");
  const [projectionExpressionNodeId, setProjectionExpressionNodeId] =
    createSignal<number>();
  const [groupByExpressionAction, setGroupByExpressionAction] = createSignal<
    GroupByExpressionAction
  >("");
  const [orderByExpressionAction, setOrderByExpressionAction] = createSignal<
    OrderByExpressionAction
  >("");
  const [orderDirection, setOrderDirection] = createSignal<OrderDirection>(
    "asc",
  );
  const [nullsOrder, setNullsOrder] = createSignal<NullsOrder>("");
  const [binaryExpressionAction, setBinaryExpressionAction] = createSignal<
    BinaryExpressionAction
  >("");
  const [binaryExpressionLeftNodeId, setBinaryExpressionLeftNodeId] =
    createSignal<number>();
  const [binaryExpressionOperator, setBinaryExpressionOperator] = createSignal<
    SqlDocumentSemanticBinaryOperator | ""
  >("");
  const [binaryExpressionRightNodeId, setBinaryExpressionRightNodeId] =
    createSignal<number>();
  const [projectionItemAction, setProjectionItemAction] = createSignal<
    ProjectionItemAction
  >("");
  const [projectionMoveToIndex, setProjectionMoveToIndex] = createSignal<
    number
  >();
  const [functionArgumentAction, setFunctionArgumentAction] = createSignal<
    FunctionArgumentAction
  >("");
  const [functionArgumentIndex, setFunctionArgumentIndex] = createSignal(0);
  const [functionExpressionNodeId, setFunctionExpressionNodeId] = createSignal<
    number
  >();
  const [relationEditAction, setRelationEditAction] = createSignal<
    RelationEditAction
  >("");
  const [relationChoice, setRelationChoice] = createSignal<RelationChoice>(
    "table",
  );
  const [relationParts, setRelationParts] = createSignal<
    SqlDocumentSemanticIdentifier[]
  >([{ value: "", quote_style: null }]);
  const [relationAlias, setRelationAlias] = createSignal<
    SqlDocumentSemanticIdentifier
  >({ value: "", quote_style: null });
  const [derivedRelationQueryNodeId, setDerivedRelationQueryNodeId] =
    createSignal<number>();
  const [joinOperator, setJoinOperator] = createSignal<
    SqlDocumentJoinOperator
  >("inner");
  const [joinConstraintChoice, setJoinConstraintChoice] = createSignal<
    JoinConstraintChoice
  >("none");
  const [joinOnExpressionNodeId, setJoinOnExpressionNodeId] = createSignal<
    number
  >();
  const [joinUsingColumns, setJoinUsingColumns] = createSignal("");
  const [columnChoice, setColumnChoice] = createSignal("");
  const [expandWildcard, setExpandWildcard] = createSignal(false);
  const [addProjectionColumn, setAddProjectionColumn] = createSignal(false);
  const [groupByItemAction, setGroupByItemAction] = createSignal<
    ListItemAction
  >("");
  const [groupByMoveToIndex, setGroupByMoveToIndex] = createSignal<number>();
  const [orderByItemAction, setOrderByItemAction] = createSignal<
    ListItemAction
  >("");
  const [orderByMoveToIndex, setOrderByMoveToIndex] = createSignal<number>();
  const [filterExpressionNodeId, setFilterExpressionNodeId] = createSignal<
    number
  >();
  const [filterAction, setFilterAction] = createSignal<"set" | "remove" | "">(
    "",
  );
  const [addHavingClause, setAddHavingClause] = createSignal(false);
  const [aggregateFunction, setAggregateFunction] = createSignal<
    SqlDocumentAggregateFunction | ""
  >("");
  const [aggregateDistinct, setAggregateDistinct] = createSignal(false);
  const [aggregateAction, setAggregateAction] = createSignal<
    "set_function" | "set_distinct" | "remove" | ""
  >("");
  const [selectDistinct, setSelectDistinct] = createSignal(false);
  const [queryClauseAction, setQueryClauseAction] = createSignal<
    QueryClauseAction
  >("");
  const [limitValue, setLimitValue] = createSignal("");
  const [offsetValue, setOffsetValue] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  let treeRoot: HTMLDivElement | undefined;
  let generation = 0;

  const nodeById = createMemo(() => {
    const byId = new Map<number, SqlDocumentNode>();
    for (const node of document()?.nodes ?? []) byId.set(node.id, node);
    return byId;
  });
  const childrenByParent = createMemo(() => {
    const result = new Map<number | null, SqlDocumentNode[]>();
    for (const node of document()?.nodes ?? []) {
      const children = result.get(node.parent) ?? [];
      children.push(node);
      result.set(node.parent, children);
    }
    return result;
  });
  const selectedNode = () => nodeById().get(selectedNodeId() ?? -1);
  const roots = () => childrenByParent().get(null) ?? [];
  const selectedProjectionItems = () => {
    const node = selectedNode();
    if (node?.kind !== "select_item" || node.parent == null) return [];
    return (childrenByParent().get(node.parent) ?? [])
      .filter((child) => child.kind === "select_item")
      .sort((left, right) =>
        (left.range?.start ?? Number.MAX_SAFE_INTEGER) -
        (right.range?.start ?? Number.MAX_SAFE_INTEGER)
      );
  };
  const selectedProjectionItemIndex = () =>
    selectedProjectionItems().findIndex((item) => item.id === selectedNodeId());
  const listItems = (
    node: SqlDocumentNode,
    kind: "group_by_item" | "order_by_item",
  ) =>
    node.parent === null ? [] : (childrenByParent().get(node.parent) ?? [])
      .filter((sibling) => sibling.kind === kind)
      .sort((left, right) =>
        (left.range?.start ?? Number.MAX_SAFE_INTEGER) -
        (right.range?.start ?? Number.MAX_SAFE_INTEGER)
      );
  const listItemIndex = (node: SqlDocumentNode) =>
    listItems(node, node.kind as "group_by_item" | "order_by_item")
      .findIndex((item) => item.id === node.id);
  const filterExpressionCandidates = (clause: SqlDocumentNode) => {
    const descendants = new Set(descendantsOf(clause).map((item) => item.id));
    const ownerScope = queryScopeId(clause);
    return (document()?.nodes ?? []).filter((candidate) =>
      isExpressionKind(candidate.kind) && candidate.range !== null &&
      !descendants.has(candidate.id) &&
      queryScopeId(candidate) === ownerScope &&
      Boolean(sourceSlice(document()?.source ?? "", candidate))
    );
  };
  const queryAncestor = (node: SqlDocumentNode) => {
    let current: SqlDocumentNode | undefined = node;
    while (current && current.kind !== "query") {
      current = current.parent === null
        ? undefined
        : nodeById().get(current.parent);
    }
    return current;
  };
  const unsignedDraft = (value: string) =>
    /^(0|[1-9][0-9]*)$/.test(value) && Number.isSafeInteger(Number(value))
      ? Number(value)
      : undefined;
  const queryScopeId = (node: SqlDocumentNode) => {
    let current: SqlDocumentNode | undefined = node;
    while (current) {
      if (current.kind === "query") return current.id;
      current = current.parent === null
        ? undefined
        : nodeById().get(current.parent);
    }
    return undefined;
  };
  const relationBindingsForQuery = (node: SqlDocumentNode) => {
    if (
      document()?.nodes.some((item) => item.kind === "common_table_expression")
    ) return [];
    const scopeId = queryScopeId(node);
    if (scopeId === undefined) return [];
    return (document()?.nodes ?? []).flatMap((factor) => {
      if (
        factor.kind !== "table_factor" || queryScopeId(factor) !== scopeId ||
        factor.semantic?.kind !== "table_factor" ||
        factor.semantic.factor_kind !== "table"
      ) return [];
      const relationNode = descendantsOf(factor).find((candidate) =>
        candidate.kind === "relation" && candidate.semantic?.kind === "relation"
      );
      if (relationNode?.semantic?.kind !== "relation") return [];
      const relationParts = relationNode.semantic.name;
      const relation = relationParts.map((part) => part.value).join(".");
      const option = (props.relationOptions?.() ?? []).find((candidate) =>
        candidate.relation === relation ||
        candidate.relation === relationParts.at(-1)?.value
      );
      if (!option) return [];
      const qualifier = factor.semantic.alias ?? relationParts.at(-1) ?? null;
      return [{ option, qualifier }];
    });
  };
  const columnChoiceOptions = (
    node: SqlDocumentNode | undefined = selectedNode(),
  ) =>
    node
      ? relationBindingsForQuery(node).flatMap((
        { option, qualifier },
        relationIndex,
      ) =>
        (option.columns ?? []).map((column, columnIndex) => ({
          id: `${relationIndex}:${columnIndex}`,
          relation: option.relation,
          label: option.label,
          column,
          reference: {
            ...(qualifier ? { qualifier: { ...qualifier } } : {}),
            column: { value: column, quote_style: '"' },
          } satisfies SqlDocumentColumnReference,
        }))
      )
      : [];
  const selectedColumnChoice = () => {
    const [relationIndex, columnIndex] = columnChoice().split(":").map(Number);
    return columnChoiceOptions().find((item) =>
      item.id === `${relationIndex}:${columnIndex}`
    );
  };
  const wildcardQualifier = (item: SqlDocumentNode) => {
    const parsed = document();
    return parsed && props.onPlanColumns
      ? wildcardProjectionQualifier(
        parsed,
        item,
        (props.relationOptions?.() ?? []).map((option) => option.relation),
      )
      : undefined;
  };
  const functionArgumentCandidates = () => {
    const currentDocument = document();
    const functionNode = selectedNode();
    if (!currentDocument || !functionNode?.range) return [];
    const allowedKinds = new Set<SqlDocumentNode["kind"]>([
      "identifier_expression",
      "compound_identifier_expression",
      "literal_expression",
      "value",
      "binary_expression",
      "unary_expression",
      "function_expression",
      "case_expression",
      "cast_expression",
      "subquery_expression",
      "predicate_expression",
      "nested_expression",
      "other_expression",
    ]);
    const ranges = new Set<string>();
    const candidates: SqlDocumentNode[] = [];
    for (const candidate of currentDocument.nodes) {
      if (!allowedKinds.has(candidate.kind) || !candidate.range) continue;
      if (
        candidate.range.start <= functionNode.range.start &&
        functionNode.range.end <= candidate.range.end
      ) continue;
      const key = `${candidate.range.start}:${candidate.range.end}`;
      if (ranges.has(key)) continue;
      const source = sourceSlice(currentDocument.source, candidate);
      if (!source) continue;
      ranges.add(key);
      candidates.push(candidate);
    }
    return candidates;
  };
  const queryExpressionCandidates = () => {
    const currentDocument = document();
    const projectionItem = selectedNode();
    if (
      !currentDocument ||
      !projectionItem ||
      (!["select_item", "select_list", "select", "query", "order_by_item"]
        .includes(
          projectionItem.kind,
        ) &&
        !expressionNodeKinds.has(projectionItem.kind))
    ) return [];
    const itemRange = projectionItem.kind === "select_item"
      ? (childrenByParent().get(projectionItem.id) ?? []).find((child) =>
        isExpressionKind(child.kind)
      )?.range ?? projectionItem.range
      : projectionItem.kind === "select_list" ||
          projectionItem.kind === "select" || projectionItem.kind === "query"
      ? undefined
      : projectionItem.range;
    const selectedQueryId = queryScopeId(projectionItem);
    const candidates: SqlDocumentNode[] = [];
    const ranges = new Set<string>();
    for (const candidate of currentDocument.nodes) {
      if (
        !isExpressionKind(candidate.kind) || !candidate.range
      ) continue;
      if (
        selectedQueryId !== undefined &&
        queryScopeId(candidate) !== selectedQueryId
      ) continue;
      if (itemRange && projectionItem.kind !== "select_list") {
        const overlaps = candidate.range.start < itemRange.end &&
          itemRange.start < candidate.range.end;
        const isExactTarget = candidate.range.start === itemRange.start &&
          candidate.range.end === itemRange.end;
        if (overlaps && !isExactTarget) continue;
      }
      const key = `${candidate.range.start}:${candidate.range.end}`;
      if (ranges.has(key)) continue;
      const source = sourceSlice(currentDocument.source, candidate);
      if (!source) continue;
      ranges.add(key);
      candidates.push(candidate);
    }
    return candidates;
  };
  const queryNodeCandidates = (target: SqlDocumentNode) => {
    const currentDocument = document();
    if (!currentDocument?.source) return [];
    const excluded = new Set([
      target.id,
      ...descendantsOf(target).map((node) => node.id),
    ]);
    const targetRange = target.range;
    return currentDocument.nodes.filter((candidate) => {
      if (candidate.id === target.id || excluded.has(candidate.id)) {
        return false;
      }
      if (!(["query", "set_operation"].includes(candidate.kind))) return false;
      if (!candidate.range) return false;
      if (
        targetRange && candidate.range.start < targetRange.end &&
        targetRange.start < candidate.range.end
      ) return false;
      return Boolean(sourceSlice(currentDocument.source, candidate));
    });
  };
  const selectedCteQuery = () => nodeById().get(cteQueryNodeId() ?? -1);
  const selectedSetOperationReplacement = () =>
    nodeById().get(setOperationReplacementNodeId() ?? -1);
  const cteSiblings = (node: SqlDocumentNode) =>
    node.parent === null ? [] : (childrenByParent().get(node.parent) ?? [])
      .filter((sibling) => sibling.kind === "common_table_expression")
      .sort((left, right) =>
        (left.range?.start ?? Number.MAX_SAFE_INTEGER) -
        (right.range?.start ?? Number.MAX_SAFE_INTEGER)
      );
  const cteSiblingIndex = (node: SqlDocumentNode) =>
    cteSiblings(node).findIndex((sibling) => sibling.id === node.id);
  const descendantsOf = (node: SqlDocumentNode): SqlDocumentNode[] => {
    const found: SqlDocumentNode[] = [];
    const visit = (parentId: number) => {
      for (const child of childrenByParent().get(parentId) ?? []) {
        found.push(child);
        visit(child.id);
      }
    };
    visit(node.id);
    return found;
  };
  const relationExpressionCandidates = (node: SqlDocumentNode) => {
    const currentDocument = document();
    if (!currentDocument) return [];
    const selectedQueryId = queryScopeId(node);
    const ranges = new Set<string>();
    const candidates: SqlDocumentNode[] = [];
    for (const candidate of currentDocument.nodes) {
      if (
        !expressionNodeKinds.has(candidate.kind) || !candidate.range ||
        (selectedQueryId !== undefined &&
          queryScopeId(candidate) !== selectedQueryId)
      ) continue;
      const key = `${candidate.range.start}:${candidate.range.end}`;
      if (ranges.has(key)) continue;
      const source = sourceSlice(currentDocument.source, candidate);
      if (!source) continue;
      ranges.add(key);
      candidates.push(candidate);
    }
    return candidates;
  };
  const derivedRelationQueryCandidates = (node: SqlDocumentNode) => {
    const currentDocument = document();
    if (!currentDocument) return [];
    const ownerQueryId = queryScopeId(node);
    return currentDocument.nodes.filter((candidate) =>
      candidate.kind === "query" && candidate.id !== ownerQueryId &&
      candidate.range !== null && sourceSlice(currentDocument.source, candidate)
          .length > 0
    );
  };
  const relationFactorFromDraft = (): SqlDocumentRelationFactor | undefined => {
    if (relationChoice() === "derived") {
      const queryNodeId = derivedRelationQueryNodeId();
      const alias = relationAlias();
      if (queryNodeId === undefined || !alias.value.trim()) return undefined;
      return {
        kind: "derived",
        query_node_id: queryNodeId,
        alias: { ...alias, value: alias.value.trim() },
      };
    }
    const name = relationParts().map((part) => ({
      ...part,
      value: part.value.trim(),
    }));
    if (name.length === 0 || name.some((part) => !part.value)) {
      return undefined;
    }
    const alias = relationAlias().value.trim()
      ? { ...relationAlias(), value: relationAlias().value.trim() }
      : null;
    return { kind: "table", name, alias };
  };
  const currentRelationOptionValue = () =>
    relationParts().length === 1 ? relationParts()[0].value : "";
  const availableRelationOptions = () => {
    const options = props.relationOptions?.() ?? [];
    const current = currentRelationOptionValue();
    return current && !options.some((option) => option.relation === current)
      ? [{ relation: current, label: current }, ...options]
      : options;
  };
  const joinRequiresNoConstraint = () =>
    joinOperator() === "cross" ||
    joinOperator().startsWith("natural");
  const relationJoinConstraint = (
    node: SqlDocumentNode,
  ): SqlDocumentJoinConstraint | undefined => {
    if (joinRequiresNoConstraint()) return { kind: "none" };
    if (joinConstraintChoice() === "none") return { kind: "none" };
    if (joinConstraintChoice() === "on") {
      const expression = relationExpressionCandidates(node).find((item) =>
        item.id === joinOnExpressionNodeId()
      );
      return expression
        ? {
          kind: "on",
          expression_node_id: expression.id,
          expression_expected_kind: expression.kind,
        }
        : undefined;
    }
    const columns = joinUsingColumns().split(",").map((column) => column.trim())
      .filter(Boolean);
    if (columns.length === 0) return undefined;
    return {
      kind: "using",
      columns: columns.map((value) => ({ value, quote_style: null })),
    };
  };
  const relationSemanticEdit = (
    node: SqlDocumentNode,
  ): SqlDocumentSemanticEdit | undefined => {
    if (relationEditAction() === "add_from" && node.kind === "select") {
      const factor = relationFactorFromDraft();
      return factor
        ? {
          kind: "add_from_relation",
          node_id: node.id,
          expected_kind: "select",
          factor,
        }
        : undefined;
    }
    if (relationEditAction() === "replace" && node.kind === "table_factor") {
      const factor = relationFactorFromDraft();
      return factor
        ? {
          kind: "replace_table_factor",
          node_id: node.id,
          expected_kind: "table_factor",
          factor,
        }
        : undefined;
    }
    if (
      (relationEditAction() === "add_join" &&
        node.kind === "relation_tree") ||
      (relationEditAction() === "replace_join" && node.kind === "join")
    ) {
      const relation = relationFactorFromDraft();
      const constraint = relationJoinConstraint(node);
      if (!relation || !constraint) return undefined;
      const join = { operator: joinOperator(), relation, constraint };
      return relationEditAction() === "add_join"
        ? {
          kind: "add_join",
          node_id: node.id,
          expected_kind: "relation_tree",
          join,
        }
        : {
          kind: "replace_join",
          node_id: node.id,
          expected_kind: "join",
          join,
        };
    }
    return undefined;
  };
  const markRelationEdit = (node: SqlDocumentNode) => {
    setRelationEditAction(
      node.kind === "relation_tree"
        ? "add_join"
        : node.kind === "join"
        ? "replace_join"
        : node.kind === "table_factor"
        ? "replace"
        : node.kind === "select"
        ? "add_from"
        : "",
    );
  };
  const selectedProjectionExpression = () =>
    nodeById().get(projectionExpressionNodeId() ?? -1);
  const selectedBinaryExpressionLeftNode = () =>
    nodeById().get(binaryExpressionLeftNodeId() ?? -1);
  const selectedBinaryExpressionRightNode = () =>
    nodeById().get(binaryExpressionRightNodeId() ?? -1);
  const selectedCaseOperandNode = () =>
    nodeById().get(caseOperandNodeId() ?? -1);
  const selectHasWhereClause = (node: SqlDocumentNode) =>
    (childrenByParent().get(node.id) ?? []).some((child) =>
      child.kind === "where_clause"
    );
  const selectHasHavingClause = (node: SqlDocumentNode) =>
    (childrenByParent().get(node.id) ?? []).some((child) =>
      child.kind === "having_clause"
    );
  const queryHasClause = (
    node: SqlDocumentNode,
    kind: "limit_clause" | "offset_clause",
  ) =>
    (childrenByParent().get(node.id) ?? []).some((child) =>
      child.kind === kind || (kind === "offset_clause" &&
        (childrenByParent().get(child.id) ?? []).some((nested) =>
          nested.kind === kind
        ))
    );
  const queryClauseNode = (
    node: SqlDocumentNode,
    kind: "limit_clause" | "offset_clause",
  ) => {
    const query = node.kind === "query" ? node : queryAncestor(node);
    if (!query) return undefined;
    return descendantsOf(query).find((candidate) => candidate.kind === kind);
  };
  const queryClauseValue = (
    node: SqlDocumentNode,
    kind: "limit_clause" | "offset_clause",
  ) => {
    const clause = queryClauseNode(node, kind);
    return clause?.semantic?.kind === "query_clause_value" &&
        clause.semantic.value !== null
      ? String(clause.semantic.value)
      : "";
  };
  const selectedFilterExpression = () =>
    nodeById().get(filterExpressionNodeId() ?? -1);
  const currentProjectionExpression = () => {
    const item = selectedNode();
    if (item?.kind !== "select_item") return undefined;
    return (childrenByParent().get(item.id) ?? []).find((child) =>
      isExpressionKind(child.kind)
    );
  };
  const isExpressionKind = (kind: SqlDocumentNode["kind"]) =>
    expressionNodeKinds.has(kind);
  const isInsideProjectionItem = (node: SqlDocumentNode) => {
    let current: SqlDocumentNode | undefined = node;
    const nodes = nodeById();
    while (current) {
      if (current.kind === "select_item") return true;
      if (current.kind === "select" || current.kind === "query") return false;
      current = current.parent === null ? undefined : nodes.get(current.parent);
    }
    return false;
  };
  const selectedFunctionExpression = () =>
    nodeById().get(functionExpressionNodeId() ?? -1);
  const hasFunctionArgumentInspector = (node: SqlDocumentNode) =>
    node.kind === "function_expression" &&
    node.semantic?.kind === "function" &&
    node.semantic.argument_count !== null &&
    (node.semantic.can_replace_argument || node.semantic.can_add_argument ||
      node.semantic.can_remove_argument) &&
    !node.semantic.has_filter &&
    !node.semantic.has_over &&
    !node.semantic.has_argument_clauses;
  const selectedOperator = () => {
    const node = selectedNode();
    return node?.semantic?.kind === "binary_operator"
      ? node.semantic.operator
      : "";
  };
  const currentOperatorSupported = () =>
    Boolean(operatorKind(selectedOperator()));
  const editorCanApply = () => {
    const node = selectedNode();
    if (!node || props.disabled?.() || busy()) return false;
    if (node.kind === "select_item" && expandWildcard()) {
      return node.semantic?.kind === "wildcard" && node.range !== null &&
        !!wildcardQualifier(node);
    }
    if (node.kind === "select_list" && addProjectionColumn()) {
      return node.range !== null && Boolean(selectedColumnChoice());
    }
    if (node.kind === "select" && addHavingClause()) {
      return node.range !== null && !selectHasHavingClause(node);
    }
    if (
      node.kind === "select" && node.semantic?.kind === "select" &&
      selectDistinct() !== node.semantic.distinct
    ) {
      return node.range !== null && !node.semantic.distinct_on;
    }
    if (node.kind === "group_by_item") {
      if (groupByItemAction() === "remove") return node.range !== null;
      if (groupByItemAction() === "move") {
        const items = listItems(node, "group_by_item");
        return node.range !== null && groupByMoveToIndex() !== undefined &&
          groupByMoveToIndex()! >= 0 && groupByMoveToIndex()! < items.length &&
          groupByMoveToIndex() !== listItemIndex(node);
      }
    }
    if (node.kind === "order_by_item") {
      if (orderByItemAction() === "remove") return node.range !== null;
      if (orderByItemAction() === "move") {
        const items = listItems(node, "order_by_item");
        return node.range !== null && orderByMoveToIndex() !== undefined &&
          orderByMoveToIndex()! >= 0 && orderByMoveToIndex()! < items.length &&
          orderByMoveToIndex() !== listItemIndex(node);
      }
      if (node.semantic?.kind === "order_by_item") {
        return node.range !== null && (
          (orderDirection() !== (node.semantic.direction ?? "asc")) ||
          ((nullsOrder() || null) !== node.semantic.nulls_order) ||
          Boolean(
            selectedProjectionExpression() &&
              selectedProjectionExpression()?.id !==
                (childrenByParent().get(node.id) ?? []).find((child) =>
                  isExpressionKind(child.kind)
                )?.id,
          )
        );
      }
    }
    if (node.kind === "where_clause" || node.kind === "having_clause") {
      if (filterAction() === "remove") return node.range !== null;
      if (filterAction() === "set") {
        const expression = selectedFilterExpression();
        return node.range !== null && Boolean(expression?.range) &&
          queryScopeId(expression!) === queryScopeId(node);
      }
    }
    if (node.kind === "function_expression" && aggregateFunctionKind(node)) {
      const current = aggregateFunctionKind(node);
      if (aggregateAction() === "remove") return node.range !== null;
      if (aggregateAction() === "set_function") {
        return node.range !== null && Boolean(aggregateFunction()) &&
          aggregateFunction() !== current;
      }
      if (aggregateAction() === "set_distinct") {
        return node.range !== null && node.semantic?.kind === "function" &&
          aggregateDistinct() !== node.semantic.distinct;
      }
    }
    if (
      node.kind === "query" || node.kind === "limit_clause" ||
      node.kind === "offset_clause"
    ) {
      const query = node.kind === "query" ? node : queryAncestor(node);
      if (!query?.range) return false;
      if (queryClauseAction() === "set_limit") {
        return unsignedDraft(limitValue()) !== undefined;
      }
      if (queryClauseAction() === "set_offset") {
        return unsignedDraft(offsetValue()) !== undefined;
      }
      if (queryClauseAction() === "remove_limit") {
        return queryHasClause(query, "limit_clause");
      }
      if (queryClauseAction() === "remove_offset") {
        return queryHasClause(query, "offset_clause");
      }
    }
    if (
      relationSemanticEdit(node) &&
      (node.kind === "table_factor" || node.kind === "relation_tree" ||
        node.kind === "join" || node.kind === "select")
    ) return node.range !== null;
    if (
      (subqueryPredicateTemplate() || replaceWithExists()) &&
      isExpressionKind(node.kind)
    ) {
      return node.range !== null && node.kind !== "other_expression";
    }
    if (predicateTemplate() && isExpressionKind(node.kind)) {
      return node.range !== null;
    }
    if (caseBranchAction() === "wrap" && isExpressionKind(node.kind)) {
      return node.range !== null;
    }
    if (addWhereClause() && node.kind === "select") {
      return node.range !== null && !selectHasWhereClause(node);
    }
    if (wrapAggregateFunction() && isExpressionKind(node.kind)) {
      return isInsideProjectionItem(node) && node.range !== null;
    }
    if (wrapCastTargetType() && isExpressionKind(node.kind)) {
      return node.kind !== "other_expression" &&
        node.kind !== "subquery_expression" && node.range !== null;
    }
    if (
      expressionLeafAction() === "identifier" &&
      isExpressionKind(node.kind)
    ) {
      return parts().length > 0 &&
        parts().every((part) => part.value.trim().length > 0);
    }
    if (expressionLeafAction() === "literal" && isExpressionKind(node.kind)) {
      if (literal().kind === "null" || literal().kind === "boolean") {
        return true;
      }
      return Boolean(literal().kind && literal().value.trim());
    }
    if (node.kind === "select" && groupByExpressionAction() === "add") {
      return Boolean(selectedProjectionExpression());
    }
    if (
      isExpressionKind(node.kind) && binaryExpressionAction() === "replace"
    ) {
      const expression = selectedProjectionExpression();
      return Boolean(expression && expression.id !== node.id);
    }
    if (
      (node.kind === "select_item" || isExpressionKind(node.kind)) &&
      binaryExpressionAction() === "compose"
    ) {
      return Boolean(
        selectedBinaryExpressionLeftNode() &&
          binaryExpressionOperator() &&
          selectedBinaryExpressionRightNode(),
      );
    }
    if (
      node.kind === "identifier_expression" ||
      node.kind === "compound_identifier_expression"
    ) {
      return parts().length > 0 &&
        parts().every((part) => part.value.trim().length > 0);
    }
    if (node.kind === "value" || node.kind === "literal_expression") {
      if (literal().kind === "null" || literal().kind === "boolean") {
        return true;
      }
      return Boolean(literal().kind && literal().value.trim());
    }
    if (node.kind === "set_operation") {
      const semantic = node.semantic;
      if (semantic?.kind !== "set_operation") return false;
      if (setOperationKeep()) return node.range !== null;
      return setOperationOperator() !== "" && setOperationQuantifier() !== "" &&
        (setOperationOperator() !== setOperatorKind(semantic.operator) ||
          setOperationQuantifier() !== setQuantifierKind(semantic.quantifier));
    }
    if (
      node.kind === "set_operation_left" ||
      node.kind === "set_operation_right"
    ) {
      const replacement = selectedSetOperationReplacement();
      return node.range !== null && Boolean(replacement?.range) &&
        replacement.id !== node.id;
    }
    if (node.kind === "cast_expression") {
      const currentType = node.semantic?.kind === "cast"
        ? castTypeKind(node.semantic.target_type)
        : "";
      const currentTarget = node.semantic?.kind === "cast"
        ? castTargetFrom(currentType, node.semantic.target_type)
        : undefined;
      const target = castTargetDraft();
      return isEditableCastNode(node) && node.range !== null &&
        Boolean(castTargetType()) && Boolean(target) &&
        JSON.stringify(target) !== JSON.stringify(currentTarget);
    }
    if (node.kind === "common_table_expression") {
      if (cteAction() === "remove") return node.range !== null;
      if (cteAction() === "move") {
        return node.range !== null && cteMoveToIndex() !== undefined &&
          cteMoveToIndex() !== cteSiblingIndex(node);
      }
      if (cteAction() === "replace") {
        return node.range !== null &&
          Boolean(selectedCteQuery()?.range);
      }
      const currentName = commonTableExpressionName(node);
      return node.range !== null && Boolean(cteName().value.trim()) &&
        (cteName().value !== currentName.value ||
          cteName().quote_style !== currentName.quote_style);
    }
    if (node.kind === "query") {
      return node.range !== null &&
        ((cteAction() === "add" && Boolean(cteName().value.trim())) ||
          (setOperationAdd() === "add" && Boolean(setOperationOperator())) ||
          (orderByExpressionAction() === "add" &&
            Boolean(selectedProjectionExpression())));
    }
    if (
      node.kind === "predicate_expression" &&
      node.semantic?.kind === "predicate" &&
      negatablePredicates.has(node.semantic.operator)
    ) {
      if (node.semantic.operator === "in" && inListAction() !== "") {
        return true;
      }
      return predicateIsNegated() !== node.semantic.negated;
    }
    if (
      node.kind === "unary_expression" &&
      node.semantic?.kind === "unary_operator" &&
      ["+", "-"].includes(node.semantic.operator)
    ) {
      return unarySign() !== unarySignKind(node.semantic.operator);
    }
    if (node.kind === "case_expression" && node.semantic?.kind === "case") {
      return caseBranchAction() === "add" ||
        (caseBranchAction() === "remove" && node.semantic.when_count > 1) ||
        caseHasElse() !== node.semantic.has_else;
    }
    if (node.kind === "select_item") {
      const items = selectedProjectionItems();
      const index = selectedProjectionItemIndex();
      if (projectionItemAction() === "remove") return items.length > 1;
      if (projectionItemAction() === "move") {
        return items.length > 1 && projectionMoveToIndex() !== undefined &&
          projectionMoveToIndex() !== index &&
          projectionMoveToIndex()! >= 0 &&
          projectionMoveToIndex()! < items.length;
      }
      const expression = selectedProjectionExpression();
      const currentExpression = currentProjectionExpression();
      return Boolean(
        expression && currentExpression &&
          expression.id !== currentExpression.id,
      );
    }
    if (node.kind === "select_list") {
      const selectedExpression = selectedProjectionExpression();
      const items = childrenByParent().get(node.id)?.filter((child) =>
        child.kind === "select_item"
      ) ?? [];
      if (addScalarSubquery()) {
        return items.length > 0 && items.length < 256;
      }
      return Boolean(
        selectedExpression && items.length > 0 && items.length < 256,
      );
    }
    if (
      node.kind === "function_expression" && node.semantic?.kind === "function"
    ) {
      if (functionArgumentAction() === "remove") {
        return node.semantic.can_remove_argument;
      }
      if (functionArgumentAction() === "set") {
        const count = node.semantic.argument_count;
        const expression = selectedFunctionExpression();
        if (count === null || !expression) return false;
        if (functionArgumentIndex() < count) {
          return node.semantic.can_replace_argument;
        }
        return functionArgumentIndex() === count &&
          node.semantic.can_add_argument;
      }
      return false;
    }
    return node.kind === "binary_expression" && operator() !== "";
  };

  const currentSemanticEdit = (): SqlDocumentSemanticEdit | undefined => {
    const node = selectedNode();
    if (!node) return undefined;
    if (node.kind === "select_item" && expandWildcard()) {
      if (wildcardQualifier(node)) {
        return {
          kind: "expand_projection_wildcard",
          node_id: node.id,
          expected_kind: "select_item",
          columns: [],
        };
      }
    }
    if (node.kind === "select_list" && addProjectionColumn()) {
      const column = selectedColumnChoice()?.reference;
      if (column) {
        return {
          kind: "add_projection_column",
          node_id: node.id,
          expected_kind: "select_list",
          column,
        };
      }
    }
    if (node.kind === "select" && addHavingClause()) {
      return {
        kind: "add_having_clause",
        node_id: node.id,
        expected_kind: "select",
      };
    }
    if (
      node.kind === "select" && node.semantic?.kind === "select" &&
      selectDistinct() !== node.semantic.distinct && !node.semantic.distinct_on
    ) {
      return {
        kind: "set_select_distinct",
        node_id: node.id,
        expected_kind: "select",
        distinct: selectDistinct(),
      };
    }
    if (node.kind === "group_by_item") {
      if (groupByItemAction() === "remove") {
        return {
          kind: "remove_group_by_item",
          node_id: node.id,
          expected_kind: "group_by_item",
        };
      }
      if (
        groupByItemAction() === "move" && groupByMoveToIndex() !== undefined
      ) {
        return {
          kind: "move_group_by_item",
          node_id: node.id,
          expected_kind: "group_by_item",
          to_index: groupByMoveToIndex()!,
        };
      }
    }
    if (node.kind === "order_by_item") {
      if (orderByItemAction() === "remove") {
        return {
          kind: "remove_order_by_item",
          node_id: node.id,
          expected_kind: "order_by_item",
        };
      }
      if (
        orderByItemAction() === "move" && orderByMoveToIndex() !== undefined
      ) {
        return {
          kind: "move_order_by_item",
          node_id: node.id,
          expected_kind: "order_by_item",
          to_index: orderByMoveToIndex()!,
        };
      }
      const expression = selectedProjectionExpression();
      const currentExpression = (childrenByParent().get(node.id) ?? []).find((
        child,
      ) => isExpressionKind(child.kind));
      if (expression && expression.id !== currentExpression?.id) {
        return {
          kind: "set_order_by_expression",
          node_id: node.id,
          expected_kind: "order_by_item",
          expression_node_id: expression.id,
          expression_expected_kind: expression.kind,
        };
      }
      if (node.semantic?.kind === "order_by_item") {
        return {
          kind: "set_order_by_options",
          node_id: node.id,
          expected_kind: "order_by_item",
          direction: orderDirection(),
          nulls_order: nullsOrder() || null,
        };
      }
    }
    if (node.kind === "where_clause" || node.kind === "having_clause") {
      if (filterAction() === "remove") {
        return {
          kind: node.kind === "having_clause"
            ? "remove_having_clause"
            : "remove_filter_clause",
          node_id: node.id,
          expected_kind: node.kind,
        };
      }
      const expression = selectedFilterExpression();
      if (filterAction() === "set" && expression) {
        return {
          kind: "set_filter_predicate",
          node_id: node.id,
          expected_kind: node.kind,
          expression_node_id: expression.id,
          expression_expected_kind: expression.kind,
        };
      }
    }
    if (node.kind === "function_expression" && aggregateFunctionKind(node)) {
      if (aggregateAction() === "remove") {
        return {
          kind: "remove_aggregate",
          node_id: node.id,
          expected_kind: "function_expression",
        };
      }
      if (aggregateAction() === "set_function" && aggregateFunction()) {
        return {
          kind: "set_aggregate_function",
          node_id: node.id,
          expected_kind: "function_expression",
          function: aggregateFunction()!,
        };
      }
      if (aggregateAction() === "set_distinct") {
        return {
          kind: "set_aggregate_distinct",
          node_id: node.id,
          expected_kind: "function_expression",
          distinct: aggregateDistinct(),
        };
      }
    }
    if (
      node.kind === "query" || node.kind === "limit_clause" ||
      node.kind === "offset_clause"
    ) {
      const query = node.kind === "query" ? node : queryAncestor(node);
      if (!query) return undefined;
      if (queryClauseAction() === "set_limit") {
        const value = unsignedDraft(limitValue());
        if (value !== undefined) {
          return {
            kind: "set_limit_value",
            node_id: query.id,
            expected_kind: "query",
            value,
          };
        }
      }
      if (queryClauseAction() === "remove_limit") {
        return {
          kind: "remove_limit",
          node_id: query.id,
          expected_kind: "query",
        };
      }
      if (queryClauseAction() === "set_offset") {
        const value = unsignedDraft(offsetValue());
        if (value !== undefined) {
          return {
            kind: "set_offset_value",
            node_id: query.id,
            expected_kind: "query",
            value,
          };
        }
      }
      if (queryClauseAction() === "remove_offset") {
        return {
          kind: "remove_offset",
          node_id: query.id,
          expected_kind: "query",
        };
      }
    }
    if (
      subqueryPredicateTemplate() && isExpressionKind(node.kind)
    ) {
      return {
        kind: "wrap_expression_in_subquery_predicate",
        node_id: node.id,
        expected_kind: node.kind,
        template: subqueryPredicateTemplate()!,
      };
    }
    if (replaceWithExists() && isExpressionKind(node.kind)) {
      return {
        kind: "replace_expression_with_exists_subquery",
        node_id: node.id,
        expected_kind: node.kind,
      };
    }
    if (
      node.kind === "set_operation_left" ||
      node.kind === "set_operation_right"
    ) {
      const expression = selectedSetOperationReplacement();
      if (expression) {
        return {
          kind: "replace_set_operation_operand",
          node_id: node.id,
          expected_kind: node.kind,
          expression_node_id: expression.id,
          expression_expected_kind: expression.kind,
        };
      }
    }
    if (node.kind === "common_table_expression") {
      if (cteAction() === "replace") {
        const query = selectedCteQuery();
        if (query) {
          return {
            kind: "replace_common_table_expression_query",
            node_id: node.id,
            expected_kind: "common_table_expression",
            query_node_id: query.id,
          };
        }
      }
      if (cteAction() === "move" && cteMoveToIndex() !== undefined) {
        return {
          kind: "move_common_table_expression",
          node_id: node.id,
          expected_kind: "common_table_expression",
          to_index: cteMoveToIndex()!,
        };
      }
    }
    const relationEdit = relationSemanticEdit(node);
    if (relationEdit) return relationEdit;
    return semanticEdit(
      node,
      expressionLeafAction(),
      parts(),
      literal(),
      operator(),
      setOperationOperator(),
      setOperationQuantifier(),
      setOperationAdd(),
      setOperationKeep(),
      castTargetDraft(),
      wrapCastTargetType(),
      wrapAggregateFunction(),
      wrapAggregateDistinct(),
      cteName(),
      cteAction(),
      predicateIsNegated(),
      unarySign(),
      caseBranchAction(),
      caseHasElse(),
      caseOperandAction(),
      selectedCaseOperandNode(),
      selectedProjectionExpression(),
      binaryExpressionAction(),
      selectedBinaryExpressionLeftNode(),
      binaryExpressionOperator(),
      selectedBinaryExpressionRightNode(),
      functionArgumentAction(),
      functionArgumentIndex(),
      selectedFunctionExpression(),
      groupByExpressionAction(),
      projectionItemAction(),
      projectionMoveToIndex(),
      orderByExpressionAction(),
      orderDirection(),
      nullsOrder(),
      addWhereClause(),
      predicateTemplate(),
      inListAction(),
      addScalarSubquery(),
    );
  };

  createEffect(() => {
    const source = props.sql();
    props.scope?.();
    const currentGeneration = ++generation;
    setLoading(true);
    setBusy(false);
    setError(undefined);
    setDocument(undefined);
    setSelectedNodeId(undefined);
    void parseSqlDocument(source).then((parsed) => {
      if (currentGeneration !== generation || props.sql() !== source) return;
      setDocument(parsed);
      setLoading(false);
    }).catch(() => {
      if (currentGeneration !== generation) return;
      setLoading(false);
      setError(t("sqlPage.structure.documentParseFailed"));
    });
  });

  const selectNode = (node: SqlDocumentNode) => {
    setSelectedNodeId(node.id);
    setParts(identifierParts(node));
    setLiteral(initialLiteralDraft(node));
    setExpressionLeafAction(
      node.kind === "identifier_expression" ||
        node.kind === "compound_identifier_expression"
        ? "identifier"
        : node.semantic?.kind === "literal"
        ? "literal"
        : "",
    );
    setOperator(
      node.semantic?.kind === "binary_operator"
        ? operatorKind(node.semantic.operator) ?? ""
        : "",
    );
    setSetOperationOperator(
      node.semantic?.kind === "set_operation"
        ? setOperatorKind(node.semantic.operator)
        : "",
    );
    setSetOperationQuantifier(
      node.semantic?.kind === "set_operation"
        ? setQuantifierKind(node.semantic.quantifier)
        : "default",
    );
    setSetOperationAdd("");
    setSetOperationKeep("");
    setSetOperationReplacementNodeId(undefined);
    setCastTargetType(
      node.semantic?.kind === "cast"
        ? castTypeKind(node.semantic.target_type)
        : "",
    );
    setCastTargetDraft(
      node.semantic?.kind === "cast"
        ? castTargetFrom(
          castTypeKind(node.semantic.target_type),
          node.semantic.target_type,
        )
        : undefined,
    );
    setWrapCastTargetType("");
    setWrapAggregateFunction("");
    setWrapAggregateDistinct(false);
    setCteName(commonTableExpressionName(node));
    setCteAction("");
    setCteQueryNodeId(undefined);
    setCteMoveToIndex(undefined);
    if (node.kind === "query") {
      setCteName({ value: "new_cte", quote_style: null });
      setSetOperationOperator("union");
      setSetOperationQuantifier("default");
    }
    setPredicateIsNegated(
      node.semantic?.kind === "predicate" ? node.semantic.negated : false,
    );
    setUnarySign(
      node.semantic?.kind === "unary_operator"
        ? unarySignKind(node.semantic.operator)
        : "",
    );
    setCaseBranchAction("");
    setCaseOperandAction("");
    setCaseOperandNodeId(undefined);
    setAddWhereClause(false);
    setPredicateTemplate("");
    setSubqueryPredicateTemplate("");
    setReplaceWithExists(false);
    setInListAction("");
    setCaseHasElse(
      node.semantic?.kind === "case" ? node.semantic.has_else : false,
    );
    if (node.kind === "case_expression" && node.semantic?.kind === "case" && node.semantic.has_operand) {
      setCaseOperandNodeId((childrenByParent().get(node.id) ?? []).find((child) => isExpressionKind(child.kind))?.id);
    }
    setProjectionExpressionNodeId(undefined);
    setGroupByExpressionAction("");
    setOrderByExpressionAction("");
    setOrderDirection(
      node.semantic?.kind === "order_by_item"
        ? node.semantic.direction ?? "asc"
        : "asc",
    );
    setNullsOrder(
      node.semantic?.kind === "order_by_item"
        ? node.semantic.nulls_order ?? ""
        : "",
    );
    setBinaryExpressionAction("");
    setBinaryExpressionLeftNodeId(undefined);
    setBinaryExpressionOperator("");
    setBinaryExpressionRightNodeId(undefined);
    setProjectionItemAction("");
    setProjectionMoveToIndex(undefined);
    setFunctionArgumentAction("");
    setFunctionArgumentIndex(0);
    setFunctionExpressionNodeId(undefined);
    setRelationEditAction("");
    setRelationChoice("table");
    setDerivedRelationQueryNodeId(undefined);
    setJoinOperator("inner");
    setJoinConstraintChoice("none");
    setJoinOnExpressionNodeId(undefined);
    setJoinUsingColumns("");
    setColumnChoice("");
    setExpandWildcard(false);
    setAddProjectionColumn(false);
    setGroupByItemAction("");
    setGroupByMoveToIndex(undefined);
    setOrderByItemAction("");
    setOrderByMoveToIndex(undefined);
    setFilterExpressionNodeId(undefined);
    setFilterAction("");
    setAddHavingClause(false);
    setAggregateFunction(aggregateFunctionKind(node));
    setAggregateDistinct(
      node.semantic?.kind === "function" ? node.semantic.distinct : false,
    );
    setAggregateAction("");
    setSelectDistinct(
      node.semantic?.kind === "select" ? node.semantic.distinct : false,
    );
    setQueryClauseAction("");
    setLimitValue(queryClauseValue(node, "limit_clause"));
    setOffsetValue(queryClauseValue(node, "offset_clause"));
    const relationTarget = node.kind === "table_factor"
      ? node
      : node.kind === "join"
      ? descendantsOf(node).filter((candidate) =>
        candidate.kind === "table_factor"
      ).at(-1)
      : undefined;
    const relationNode = relationTarget
      ? descendantsOf(relationTarget).find((candidate) =>
        candidate.kind === "relation" &&
        candidate.semantic?.kind === "relation"
      )
      : undefined;
    const selectedRelationParts = relationNode?.semantic?.kind === "relation"
      ? relationNode.semantic.name.map((part) => ({ ...part }))
      : [{ value: "", quote_style: null }];
    setRelationParts(
      selectedRelationParts.length > 0
        ? selectedRelationParts
        : [{ value: "", quote_style: null }],
    );
    const targetAlias = relationTarget?.semantic?.kind === "table_factor"
      ? relationTarget.semantic.alias
      : null;
    setRelationAlias(
      targetAlias ? { ...targetAlias } : { value: "", quote_style: null },
    );
    setError(undefined);
  };

  createEffect(() => {
    const currentDocument = document();
    const preferredKinds = props.initialNodeKinds?.() ?? [];
    if (!currentDocument || preferredKinds.length === 0) return;
    const selectedId = untrack(selectedNodeId);
    const candidate = preferredKinds.map((kind) =>
      currentDocument.nodes.find((node) =>
        node.kind === kind && node.range !== null
      )
    ).find((node) => node !== undefined);
    if (candidate && candidate.id !== selectedId) selectNode(candidate);
  });

  const focusTreeNode = (node: SqlDocumentNode) => {
    const target = treeRoot?.querySelector<HTMLButtonElement>(
      `[data-sql-document-node="${node.id}"]`,
    );
    if (!target) return;
    target.focus();
    selectNode(node);
  };

  const onTreeKeyDown = (event: KeyboardEvent) => {
    const target = event.target;
    if (!(target instanceof HTMLButtonElement)) return;
    const id = Number(target.dataset.sqlDocumentNode);
    const currentDocument = document();
    const current = currentDocument?.nodes.find((node) => node.id === id);
    if (!current || !currentDocument) return;
    const nodes = currentDocument.nodes;
    const index = nodes.findIndex((node) => node.id === current.id);
    let next: SqlDocumentNode | undefined;
    if (event.key === "ArrowDown") next = nodes[index + 1];
    else if (event.key === "ArrowUp") next = nodes[index - 1];
    else if (event.key === "Home") next = nodes[0];
    else if (event.key === "End") next = nodes.at(-1);
    else if (event.key === "ArrowRight") {
      next = childrenByParent().get(current.id)?.[0];
    } else if (event.key === "ArrowLeft" && current.parent !== null) {
      next = nodeById().get(current.parent);
    }
    if (!next) return;
    event.preventDefault();
    focusTreeNode(next);
  };

  const updatePart = (
    index: number,
    change: Partial<SqlDocumentSemanticIdentifier>,
  ) => {
    setParts((current) =>
      current.map((part, partIndex) =>
        partIndex === index ? { ...part, ...change } : part
      )
    );
  };

  onCleanup(() => {
    generation++;
  });

  const applyEdit = async (edit: SqlDocumentSemanticEdit) => {
    const currentDocument = document();
    if (!currentDocument || props.disabled?.() || busy()) return;
    const expectedScope = props.scope?.();
    const expectedSource = currentDocument.source;
    const currentGeneration = ++generation;
    setBusy(true);
    setError(undefined);
    try {
      let columnPlan: PlannedSqlColumns | undefined;
      if (edit.kind === "expand_projection_wildcard") {
        const item = currentDocument.nodes.find((node) =>
          node.id === edit.node_id
        );
        const qualifier = item && wildcardQualifier(item);
        if (!qualifier || !props.onPlanColumns) {
          throw new Error("unavailable wildcard schema");
        }
        columnPlan = await props.onPlanColumns();
        if (
          !(await columnPlan.isCurrent()) ||
          !usablePlannedColumnNames(columnPlan.columns)
        ) throw new Error("stale or ambiguous wildcard schema");
        if (
          currentGeneration !== generation || props.sql() !== expectedSource ||
          props.scope?.() !== expectedScope || props.disabled?.()
        ) return;
        edit = {
          ...edit,
          columns: columnPlan.columns.map((name) => ({
            qualifier,
            column: { value: name, quote_style: '"' },
          })),
        };
      }
      const result = await applySqlDocumentSemanticEditTransaction(
        expectedSource,
        {
          contract_version: currentDocument.contract_version,
          expected_source: expectedSource,
          edits: [edit],
        },
      );
      if (columnPlan && !(await columnPlan.isCurrent())) return;
      if (
        currentGeneration !== generation || props.sql() !== expectedSource ||
        props.scope?.() !== expectedScope || props.disabled?.()
      ) return;
      if (
        result.document.status !== "parsed" ||
        result.document.source === expectedSource
      ) {
        setError(t("sqlPage.structure.documentEditUnavailable"));
        return;
      }
      props.onSqlChange(result.document.source);
      setDocument(result.document);
      setSelectedNodeId(undefined);
      setProjectionItemAction("");
      setProjectionMoveToIndex(undefined);
      setAddScalarSubquery(false);
    } catch {
      if (currentGeneration === generation) {
        setError(t("sqlPage.structure.documentEditUnavailable"));
      }
    } finally {
      if (currentGeneration === generation) setBusy(false);
    }
  };

  const apply = () => {
    if (!selectedNode() || !editorCanApply()) return;
    const edit = currentSemanticEdit();
    if (edit) void applyEdit(edit);
  };

  const renderNode = (node: SqlDocumentNode, depth: number): JSX.Element => {
    const children = () => childrenByParent().get(node.id) ?? [];
    const siblings = () =>
      node.parent === null
        ? roots()
        : childrenByParent().get(node.parent) ?? [];
    const siblingIndex = () =>
      siblings().findIndex((item) => item.id === node.id);
    const preview = () => sourceSlice(document()?.source ?? "", node);
    const accessibleName = () =>
      preview()
        ? `${documentNodeLabel(node.kind)}: ${preview()}`
        : documentNodeLabel(node.kind);
    return (
      <div class="sql-document-tree__item" role="none">
        <button
          type="button"
          class="sql-document-tree__node"
          role="treeitem"
          aria-selected={selectedNodeId() === node.id}
          aria-level={depth + 1}
          aria-posinset={siblingIndex() + 1}
          aria-setsize={siblings().length}
          aria-expanded={children().length > 0 ? true : undefined}
          tabIndex={selectedNodeId() === node.id ||
              (selectedNodeId() === undefined && node.id === roots()[0]?.id)
            ? 0
            : -1}
          data-sql-document-node={node.id}
          style={{
            "padding-inline-start": `${8 + Math.min(depth, 12) * 12}px`,
          }}
          onClick={() => selectNode(node)}
        >
          <span class="sql-document-tree__node-kind">
            {documentNodeLabel(node.kind)}
          </span>
          <Show when={preview()}>
            <code aria-label={accessibleName()}>{preview()}</code>
          </Show>
        </button>
        <Show when={children().length > 0}>
          <div class="sql-document-tree__children" role="group">
            <For each={children()}>
              {(child) => renderNode(child, depth + 1)}
            </For>
          </div>
        </Show>
      </div>
    );
  };

  const selectedNodePreview = () => {
    const node = selectedNode();
    return node ? sourceSlice(document()?.source ?? "", node) : "";
  };

  return (
    <div class="sql-document-tree-editor">
      <Show when={loading()}>
        <span class="ui-text-muted" role="status">
          {t("sqlPage.structure.documentLoading")}
        </span>
      </Show>
      <Show when={document()}>
        {(current) => (
          <>
            <Show when={current().status !== "parsed"}>
              <p class="ui-text-warning" role="status">
                {current().diagnostics[0]?.message ??
                  t("sqlPage.structure.documentDraft")}
              </p>
            </Show>
            <Show when={current().status === "empty"}>
              <button
                type="button"
                class="ui-button ui-button-primary"
                disabled={busy() || props.disabled?.()}
                onClick={() =>
                  void applyEdit({ kind: "initialize_select", relation: null })}
              >
                {t("sqlPage.structure.startQuery")}
              </button>
            </Show>
            <div class="sql-document-tree__layout">
              <div
                class="sql-document-tree__nodes"
                role="tree"
                aria-label={t("sqlPage.structure.documentTree")}
                ref={(element) => treeRoot = element}
                onKeyDown={onTreeKeyDown}
              >
                <For each={roots()}>{(node) => renderNode(node, 0)}</For>
              </div>
              <Show when={selectedNode()}>
                {(node) => (
                  <section
                    class="sql-document-tree__editor"
                    aria-label={documentNodeLabel(node().kind)}
                  >
                    <p class="sql-document-tree__selection">
                      <strong>{documentNodeLabel(node().kind)}</strong>
                      <Show when={selectedNodePreview()}>
                        <code>{selectedNodePreview()}</code>
                      </Show>
                    </p>

                    <Show
                      when={node().kind === "table_factor" ||
                        node().kind === "relation_tree" ||
                        node().kind === "join" ||
                        node().kind === "select"}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentRelationKind")}
                          <select
                            class="ui-input"
                            value={relationChoice()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) => {
                              setRelationChoice(
                                event.currentTarget.value as RelationChoice,
                              );
                              markRelationEdit(node());
                            }}
                          >
                            <option value="table">
                              {t("sqlPage.structure.documentRelationForm")}
                            </option>
                            <option value="derived">
                              {t("sqlPage.structure.documentRelationQuery")}
                            </option>
                          </select>
                        </label>
                        <Show when={relationChoice() === "table"}>
                          <label class="ui-label">
                            {t("sqlPage.structure.documentRelationForm")}
                            <select
                              class="ui-input"
                              value={currentRelationOptionValue()}
                              disabled={busy() || props.disabled?.() ||
                                availableRelationOptions().length === 0}
                              onChange={(event) => {
                                const relation = event.currentTarget.value;
                                setRelationParts(
                                  relation
                                    ? [{ value: relation, quote_style: '"' }]
                                    : [{ value: "", quote_style: null }],
                                );
                                markRelationEdit(node());
                              }}
                            >
                              <option value="">
                                {t("sqlPage.structure.documentChooseForm")}
                              </option>
                              <For each={availableRelationOptions()}>
                                {(option) => (
                                  <option value={option.relation}>
                                    {option.label}
                                  </option>
                                )}
                              </For>
                            </select>
                          </label>
                        </Show>
                        <Show when={relationChoice() === "derived"}>
                          <label class="ui-label">
                            {t("sqlPage.structure.documentRelationQuery")}
                            <select
                              class="ui-input"
                              value={derivedRelationQueryNodeId() ?? ""}
                              disabled={busy() || props.disabled?.() ||
                                derivedRelationQueryCandidates(node())
                                    .length ===
                                  0}
                              onChange={(event) => {
                                const value = event.currentTarget.value;
                                setDerivedRelationQueryNodeId(
                                  value ? Number(value) : undefined,
                                );
                                markRelationEdit(node());
                              }}
                            >
                              <option value="">
                                {t("sqlPage.structure.documentChooseQuery")}
                              </option>
                              <For
                                each={derivedRelationQueryCandidates(node())}
                              >
                                {(candidate) => (
                                  <option value={candidate.id}>
                                    {sourceSlice(
                                      document()?.source ?? "",
                                      candidate,
                                    )}
                                  </option>
                                )}
                              </For>
                            </select>
                          </label>
                        </Show>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentRelationAlias")}
                          <input
                            class="ui-input"
                            value={relationAlias().value}
                            disabled={busy() || props.disabled?.()}
                            onInput={(event) => {
                              const value = event.currentTarget.value;
                              setRelationAlias((currentAlias) => ({
                                ...currentAlias,
                                value,
                              }));
                              markRelationEdit(node());
                            }}
                          />
                        </label>
                        <Show
                          when={node().kind === "relation_tree" ||
                            node().kind === "join"}
                        >
                          <label class="ui-label">
                            {t("sqlPage.structure.documentJoinKind")}
                            <select
                              class="ui-input"
                              value={joinOperator()}
                              disabled={busy() || props.disabled?.()}
                              onChange={(event) => {
                                const value = event.currentTarget
                                  .value as SqlDocumentJoinOperator;
                                setJoinOperator(value);
                                if (
                                  value === "cross" ||
                                  value.startsWith("natural")
                                ) {
                                  setJoinConstraintChoice("none");
                                }
                                markRelationEdit(node());
                              }}
                            >
                              <For each={joinOperators}>
                                {(option) => (
                                  <option value={option.kind}>
                                    {option.label}
                                  </option>
                                )}
                              </For>
                            </select>
                          </label>
                          <Show when={!joinRequiresNoConstraint()}>
                            <label class="ui-label">
                              {t("sqlPage.structure.documentJoinConstraint")}
                              <select
                                class="ui-input"
                                value={joinConstraintChoice()}
                                disabled={busy() || props.disabled?.()}
                                onChange={(event) => {
                                  setJoinConstraintChoice(
                                    event.currentTarget
                                      .value as JoinConstraintChoice,
                                  );
                                  markRelationEdit(node());
                                }}
                              >
                                <option value="none">
                                  {t(
                                    "sqlPage.structure.documentJoinNoConstraint",
                                  )}
                                </option>
                                <option value="on">ON</option>
                                <option value="using">USING</option>
                              </select>
                            </label>
                          </Show>
                          <Show
                            when={joinConstraintChoice() === "on" &&
                              !joinRequiresNoConstraint()}
                          >
                            <label class="ui-label">
                              {t("sqlPage.structure.documentJoinExpression")}
                              <select
                                class="ui-input"
                                value={joinOnExpressionNodeId() ?? ""}
                                disabled={busy() || props.disabled?.() ||
                                  relationExpressionCandidates(node())
                                      .length ===
                                    0}
                                onChange={(event) => {
                                  const value = event.currentTarget.value;
                                  setJoinOnExpressionNodeId(
                                    value ? Number(value) : undefined,
                                  );
                                  markRelationEdit(node());
                                }}
                              >
                                <option value="">
                                  {t(
                                    "sqlPage.structure.documentChooseExpression",
                                  )}
                                </option>
                                <For
                                  each={relationExpressionCandidates(node())}
                                >
                                  {(candidate) => (
                                    <option value={candidate.id}>
                                      {sourceSlice(
                                        document()?.source ?? "",
                                        candidate,
                                      )}
                                    </option>
                                  )}
                                </For>
                              </select>
                            </label>
                          </Show>
                          <Show
                            when={joinConstraintChoice() === "using" &&
                              !joinRequiresNoConstraint()}
                          >
                            <label class="ui-label">
                              {t("sqlPage.structure.documentJoinUsingColumns")}
                              <input
                                class="ui-input"
                                value={joinUsingColumns()}
                                disabled={busy() || props.disabled?.()}
                                onInput={(event) => {
                                  setJoinUsingColumns(
                                    event.currentTarget.value,
                                  );
                                  markRelationEdit(node());
                                }}
                              />
                            </label>
                          </Show>
                        </Show>
                        <Show
                          when={node().kind === "table_factor" ||
                            node().kind === "relation_tree" ||
                            node().kind === "join" ||
                            node().kind === "select"}
                        >
                          <IconButton
                            icon={node().kind === "relation_tree" ||
                                node().kind === "select"
                              ? "plus"
                              : "edit"}
                            label={t(
                              node().kind === "select"
                                ? "sqlPage.structure.documentAddFromRelation"
                                : node().kind === "relation_tree"
                                ? "sqlPage.structure.documentAddJoin"
                                : node().kind === "join"
                                ? "sqlPage.structure.documentReplaceJoin"
                                : "sqlPage.structure.documentReplaceRelation",
                            )}
                            disabled={busy() || props.disabled?.() ||
                              !relationFactorFromDraft() ||
                              (node().kind === "relation_tree" ||
                                  node().kind === "join"
                                ? !relationJoinConstraint(node())
                                : false)}
                            onClick={() => markRelationEdit(node())}
                          />
                        </Show>
                      </div>
                    </Show>

                    <Show when={node().kind === "query"}>
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentOrderDirection")}
                          <select
                            class="ui-input"
                            value={orderDirection()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) =>
                              setOrderDirection(
                                event.currentTarget.value as OrderDirection,
                              )}
                          >
                            <option value="asc">ASC</option>
                            <option value="desc">DESC</option>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentNullsOrder")}
                          <select
                            class="ui-input"
                            value={nullsOrder()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) =>
                              setNullsOrder(
                                event.currentTarget.value as NullsOrder,
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentNullsDefault")}
                            </option>
                            <option value="first">NULLS FIRST</option>
                            <option value="last">NULLS LAST</option>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentCteName")}
                          <input
                            class="ui-input"
                            value={cteName().value}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onInput={(event) =>
                              setCteName((current) => ({
                                ...current,
                                value: event.currentTarget.value,
                              }))}
                          />
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentQuoteStyle", {
                            index: 1,
                          })}
                          <select
                            class="ui-input"
                            value={cteName().quote_style ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onChange={(event) =>
                              setCteName((current) => ({
                                ...current,
                                quote_style: event.currentTarget.value || null,
                              }))}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentUnquoted")}
                            </option>
                            <For
                              each={identifierQuoteStyles.filter((style) =>
                                style !== null
                              )}
                            >
                              {(style) => (
                                <option value={style}>
                                  {style === "[" ? "[]" : `${style}${style}`}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="plus"
                          label={t("sqlPage.structure.documentAddCte")}
                          disabled={busy() || props.disabled?.() ||
                            !cteName().value.trim() ||
                            node().range === null}
                          onClick={() => setCteAction("add")}
                        />
                        <label class="ui-label">
                          {t("sqlPage.structure.documentSetOperator")}
                          <select
                            class="ui-input"
                            value={setOperationOperator()}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onChange={(event) =>
                              setSetOperationOperator(
                                event.currentTarget
                                  .value as SqlDocumentSetOperator | "",
                              )}
                          >
                            <option value="union">UNION</option>
                            <option value="intersect">INTERSECT</option>
                            <option value="except">EXCEPT</option>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentSetQuantifier")}
                          <select
                            class="ui-input"
                            value={setOperationQuantifier()}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onChange={(event) =>
                              setSetOperationQuantifier(
                                event.currentTarget
                                  .value as SqlDocumentSetQuantifier,
                              )}
                          >
                            <option value="default">
                              {t("sqlPage.structure.documentSetDefault")}
                            </option>
                            <option value="all">ALL</option>
                            <option value="distinct">DISTINCT</option>
                            <option value="by_name">BY NAME</option>
                            <option value="all_by_name">ALL BY NAME</option>
                            <option value="distinct_by_name">
                              DISTINCT BY NAME
                            </option>
                          </select>
                        </label>
                        <IconButton
                          icon="plus"
                          label={t("sqlPage.structure.documentAddSetOperation")}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null ||
                            !setOperationOperator() ||
                            !setOperationQuantifier()}
                          onClick={() => setSetOperationAdd("add")}
                        />
                      </div>
                    </Show>

                    <Show
                      when={isExpressionKind(node().kind)}
                      fallback={null}
                    >
                      <label class="ui-label">
                        {t("sqlPage.structure.documentExpressionLeaf")}
                        <select
                          class="ui-input"
                          value={expressionLeafAction()}
                          disabled={busy() || props.disabled?.()}
                          onChange={(event) => {
                            const action = event.currentTarget
                              .value as ExpressionLeafAction;
                            setExpressionLeafAction(action);
                            if (
                              action === "identifier" && parts().length === 0
                            ) {
                              setParts([{ value: "", quote_style: null }]);
                            }
                            if (action === "literal") {
                              setLiteral({ kind: "", value: "" });
                            }
                          }}
                        >
                          <option value="">
                            {t("sqlPage.structure.documentChooseKind")}
                          </option>
                          <option value="identifier">
                            {t("sqlPage.structure.documentIdentifier")}
                          </option>
                          <option value="literal">
                            {t("sqlPage.structure.documentLiteral")}
                          </option>
                        </select>
                      </label>
                    </Show>

                    <Show
                      when={node().kind === "identifier_expression" ||
                        node().kind === "compound_identifier_expression" ||
                        expressionLeafAction() === "identifier"}
                      fallback={null}
                    >
                      <div class="sql-document-tree__fields">
                        <For each={parts()}>
                          {(part, index) => (
                            <div class="sql-document-tree__identifier-part">
                              <label class="ui-label">
                                {t("sqlPage.structure.documentIdentifierPart", {
                                  index: index() + 1,
                                })}
                                <input
                                  class="ui-input"
                                  value={part.value}
                                  disabled={busy() || props.disabled?.()}
                                  onInput={(event) =>
                                    updatePart(index(), {
                                      value: event.currentTarget.value,
                                    })}
                                />
                              </label>
                              <label class="ui-label">
                                {t("sqlPage.structure.documentQuoteStyle", {
                                  index: index() + 1,
                                })}
                                <select
                                  class="ui-input"
                                  value={part.quote_style ?? ""}
                                  disabled={busy() || props.disabled?.()}
                                  onChange={(event) =>
                                    updatePart(index(), {
                                      quote_style: event.currentTarget.value ||
                                        null,
                                    })}
                                >
                                  <option value="">
                                    {t("sqlPage.structure.documentUnquoted")}
                                  </option>
                                  <For
                                    each={identifierQuoteStyles.filter((
                                      style,
                                    ) => style !== null)}
                                  >
                                    {(style) => (
                                      <option value={style}>
                                        {style === "["
                                          ? "[]"
                                          : `${style}${style}`}
                                      </option>
                                    )}
                                  </For>
                                  <Show
                                    when={part.quote_style &&
                                      !identifierQuoteStyles.includes(
                                        part.quote_style as
                                          | null
                                          | '"'
                                          | "`"
                                          | "[",
                                      )}
                                  >
                                    <option value={part.quote_style}>
                                      {t(
                                        "sqlPage.structure.documentCurrentQuote",
                                      )}
                                    </option>
                                  </Show>
                                </select>
                              </label>
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>

                    <Show
                      when={node().semantic?.kind === "literal" ||
                        expressionLeafAction() === "literal"}
                      fallback={null}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentValueKind")}
                          <select
                            class="ui-input"
                            value={literal().kind}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) =>
                              setLiteral((currentLiteral) => ({
                                kind: event.currentTarget
                                  .value as LiteralDraft["kind"],
                                value: currentLiteral.value,
                              }))}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseKind")}
                            </option>
                            <option value="number">
                              {t("sqlPage.structure.literal.number")}
                            </option>
                            <option value="string">
                              {t("sqlPage.structure.literal.string")}
                            </option>
                            <option value="boolean">
                              {t("sqlPage.structure.literal.boolean")}
                            </option>
                            <option value="null">
                              {t("sqlPage.structure.literal.null")}
                            </option>
                            <option value="named_parameter">
                              {t("sqlPage.structure.documentNamedParameter")}
                            </option>
                          </select>
                        </label>
                        <Show when={literal().kind === "boolean"}>
                          <label class="ui-label">
                            {t("sqlPage.structure.documentValue")}
                            <select
                              class="ui-input"
                              value={literal().value}
                              disabled={busy() || props.disabled?.()}
                              onChange={(event) =>
                                setLiteral((currentLiteral) => ({
                                  ...currentLiteral,
                                  value: event.currentTarget.value,
                                }))}
                            >
                              <option value="true">TRUE</option>
                              <option value="false">FALSE</option>
                            </select>
                          </label>
                        </Show>
                        <Show
                          when={literal().kind === "number" ||
                            literal().kind === "string" ||
                            literal().kind === "named_parameter"}
                        >
                          <label class="ui-label">
                            {literal().kind === "number"
                              ? t("sqlPage.structure.documentNumber")
                              : literal().kind === "named_parameter"
                              ? t("sqlPage.structure.documentParameterName")
                              : t("sqlPage.structure.documentString")}
                            <input
                              class="ui-input"
                              value={literal().value}
                              disabled={busy() || props.disabled?.()}
                              inputMode={literal().kind === "number"
                                ? "decimal"
                                : undefined}
                              onInput={(event) =>
                                setLiteral((currentLiteral) => ({
                                  ...currentLiteral,
                                  value: event.currentTarget.value,
                                }))}
                            />
                          </label>
                        </Show>
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "binary_expression" &&
                        node().semantic?.kind === "binary_operator"}
                    >
                      <label class="ui-label">
                        {t("sqlPage.structure.operator")}
                        <select
                          class="ui-input"
                          value={operator()}
                          disabled={busy() || props.disabled?.()}
                          onChange={(event) =>
                            setOperator(
                              event.currentTarget.value as
                                | SqlDocumentSemanticBinaryOperator
                                | "",
                            )}
                        >
                          <Show when={!currentOperatorSupported()}>
                            <option value="">{selectedOperator()}</option>
                          </Show>
                          <For each={binaryOperators}>
                            {(item) => (
                              <option value={item.kind}>{item.label}</option>
                            )}
                          </For>
                        </select>
                      </label>
                    </Show>

                    <Show
                      when={node().kind === "unary_expression" &&
                        node().semantic?.kind === "unary_operator" &&
                        ["+", "-", "NOT"].includes(node().semantic.operator.toUpperCase())}
                    >
                      <label class="ui-label">
                        {t("sqlPage.structure.documentUnarySign")}
                        <select
                          class="ui-input"
                          value={unarySign()}
                          disabled={busy() || props.disabled?.()}
                          onChange={(event) =>
                            setUnarySign(
                              event.currentTarget
                                .value as SqlDocumentUnarySign | "",
                            )}
                        >
                          <option value="positive">+</option>
                          <option value="negative">−</option>
                          <option value="not">NOT</option>
                        </select>
                      </label>
                    </Show>

                    <Show
                      when={node().kind === "set_operation" &&
                        node().semantic?.kind === "set_operation"}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentSetOperator")}
                          <select
                            class="ui-input"
                            value={setOperationOperator()}
                            disabled={busy() || props.disabled?.() ||
                              setOperationOperator() === "" ||
                              setOperationQuantifier() === ""}
                            onChange={(event) =>
                              setSetOperationOperator(
                                event.currentTarget
                                  .value as SqlDocumentSetOperator | "",
                              )}
                          >
                            <Show when={setOperationOperator() === ""}>
                              <option value="" disabled>
                                {node().semantic?.kind === "set_operation"
                                  ? t(
                                    "sqlPage.structure.documentUnsupportedSetOperator",
                                    { operator: node().semantic.operator },
                                  )
                                  : ""}
                              </option>
                            </Show>
                            <option value="union">UNION</option>
                            <option value="intersect">INTERSECT</option>
                            <option value="except">EXCEPT</option>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentSetQuantifier")}
                          <select
                            class="ui-input"
                            value={setOperationQuantifier()}
                            disabled={busy() || props.disabled?.() ||
                              setOperationOperator() === "" ||
                              setOperationQuantifier() === ""}
                            onChange={(event) =>
                              setSetOperationQuantifier(
                                event.currentTarget
                                  .value as SqlDocumentSetQuantifier,
                              )}
                          >
                            <Show when={setOperationQuantifier() === ""}>
                              <option value="" disabled>
                                {node().semantic?.kind === "set_operation"
                                  ? t(
                                    "sqlPage.structure.documentUnsupportedSetQuantifier",
                                    { quantifier: node().semantic.quantifier },
                                  )
                                  : ""}
                              </option>
                            </Show>
                            <option value="default">
                              {t("sqlPage.structure.documentSetDefault")}
                            </option>
                            <option value="all">ALL</option>
                            <option value="distinct">DISTINCT</option>
                            <option value="by_name">BY NAME</option>
                            <option value="all_by_name">ALL BY NAME</option>
                            <option value="distinct_by_name">
                              DISTINCT BY NAME
                            </option>
                          </select>
                        </label>
                        <IconButton
                          icon="trash"
                          label={t(
                            "sqlPage.structure.documentRemoveSetOperationLeft",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onClick={() => setSetOperationKeep("right")}
                        />
                        <IconButton
                          icon="trash"
                          label={t(
                            "sqlPage.structure.documentRemoveSetOperationRight",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onClick={() => setSetOperationKeep("left")}
                        />
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onClick={() => setSetOperationKeep("swap")}
                        >
                          {t("sqlPage.structure.documentSwapSetOperands")}
                        </button>
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "set_operation_left" ||
                        node().kind === "set_operation_right"}
                    >
                      <label class="ui-label">
                        {t("sqlPage.structure.documentSetOperandQuery")}
                        <select
                          class="ui-input"
                          value={setOperationReplacementNodeId() ?? ""}
                          disabled={busy() || props.disabled?.() ||
                            queryNodeCandidates(node()).length === 0}
                          onChange={(event) => {
                            const value = event.currentTarget.value;
                            setSetOperationReplacementNodeId(
                              value ? Number(value) : undefined,
                            );
                          }}
                        >
                          <option value="">
                            {t("sqlPage.structure.documentChooseQuery")}
                          </option>
                          <For each={queryNodeCandidates(node())}>
                            {(candidate) => (
                              <option value={candidate.id}>
                                {sourceSlice(
                                  document()?.source ?? "",
                                  candidate,
                                )}
                              </option>
                            )}
                          </For>
                        </select>
                      </label>
                    </Show>

                    <Show
                      when={isEditableCastNode(node())}
                    >
                      <label class="ui-label">
                        {t("sqlPage.structure.documentCastTargetType")}
                        <select
                          class="ui-input"
                          value={castTargetType()}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onChange={(event) =>
                            (() => {
                              const kind = event.currentTarget.value as
                                | SqlDocumentCastType
                                | "";
                              setCastTargetType(kind);
                              setCastTargetDraft(castTargetFrom(kind, null));
                            })()}
                        >
                          <Show when={castTargetType() === ""}>
                            <option value="" disabled>
                              {node().semantic?.kind === "cast"
                                ? `${node().semantic.target_type ?? "?"}`
                                : ""}
                            </option>
                          </Show>
                          <For each={castTypes}>
                            {(item) => (
                              <option value={item.kind}>{item.label}</option>
                            )}
                          </For>
                        </select>
                      </label>
                      <Show when={castTargetType() === "varchar"}>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentCastLength")}
                          <input class="ui-input" type="text" inputmode="numeric" pattern="[0-9]*"
                            value={castTargetDraft()?.length ?? ""}
                            disabled={busy() || props.disabled?.() || node().range === null}
                            onInput={(event) => setCastTargetDraft((target) => target && ({ ...target, length: event.currentTarget.value === "" ? null : event.currentTarget.value }))} />
                        </label>
                      </Show>
                      <Show when={castTargetType() === "decimal" || castTargetType() === "timestamp"}>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentCastPrecision")}
                          <input class="ui-input" type="text" inputmode="numeric" pattern="[0-9]*"
                            value={castTargetDraft()?.precision ?? ""}
                            disabled={busy() || props.disabled?.() || node().range === null}
                            onInput={(event) => setCastTargetDraft((target) => target && ({ ...target, precision: event.currentTarget.value === "" ? null : event.currentTarget.value }))} />
                        </label>
                      </Show>
                      <Show when={castTargetType() === "decimal"}>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentCastScale")}
                          <input class="ui-input" type="text" inputmode="numeric" pattern="-?[0-9]*"
                            value={castTargetDraft()?.scale ?? ""}
                            disabled={busy() || props.disabled?.() || node().range === null}
                            onInput={(event) => setCastTargetDraft((target) => target && ({ ...target, scale: event.currentTarget.value === "" ? null : event.currentTarget.value }))} />
                        </label>
                      </Show>
                    </Show>

                    <Show
                      when={isExpressionKind(node().kind) &&
                        node().kind !== "cast_expression" &&
                        node().kind !== "other_expression" &&
                        node().kind !== "subquery_expression"}
                    >
                      <label class="ui-label">
                        {t("sqlPage.structure.documentWrapExpressionInCast")}
                        <select
                          class="ui-input"
                          value={wrapCastTargetType()}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onChange={(event) => {
                            setWrapCastTargetType(
                              event.currentTarget.value as
                                | SqlDocumentCastType
                                | "",
                            );
                            setExpressionLeafAction("");
                          }}
                        >
                          <option value=""></option>
                          <For each={castTypes}>
                            {(item) => (
                              <option value={item.kind}>{item.label}</option>
                            )}
                          </For>
                        </select>
                      </label>
                    </Show>

                    <Show
                      when={isExpressionKind(node().kind) &&
                        isInsideProjectionItem(node())}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentAggregateFunction")}
                          <select
                            class="ui-input"
                            value={wrapAggregateFunction()}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onChange={(event) => {
                              setWrapAggregateFunction(
                                event.currentTarget
                                  .value as SqlDocumentAggregateFunction | "",
                              );
                              setExpressionLeafAction("");
                              setWrapCastTargetType("");
                            }}
                          >
                            <option value=""></option>
                            <For each={aggregateFunctions}>
                              {(item) => (
                                <option value={item.kind}>{item.label}</option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label class="ui-label sql-document-tree__negation">
                          <input
                            type="checkbox"
                            checked={wrapAggregateDistinct()}
                            disabled={busy() || props.disabled?.() ||
                              !wrapAggregateFunction()}
                            onChange={(event) =>
                              setWrapAggregateDistinct(
                                event.currentTarget.checked,
                              )}
                          />
                          {t("sqlPage.structure.documentAggregateDistinct")}
                        </label>
                      </div>
                    </Show>

                    <Show when={isExpressionKind(node().kind)}>
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentPredicateTemplate")}
                          <select
                            class="ui-input"
                            value={predicateTemplate()}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onChange={(event) =>
                              setPredicateTemplate(
                                event.currentTarget.value as
                                  | SqlDocumentPredicateTemplate
                                  | "",
                              )}
                          >
                            <option value=""></option>
                            <For each={predicateTemplates}>
                              {(item) => (
                                <option value={item.kind}>{item.label}</option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentSubqueryPredicate")}
                          <select
                            class="ui-input"
                            value={subqueryPredicateTemplate()}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null ||
                              node().kind === "other_expression"}
                            onChange={(event) =>
                              setSubqueryPredicateTemplate(
                                event.currentTarget.value as
                                  | SqlDocumentSubqueryPredicateTemplate
                                  | "",
                              )}
                          >
                            <option value=""></option>
                            <option value="in">IN (SELECT 1)</option>
                            <option value="not_in">NOT IN (SELECT 1)</option>
                          </select>
                        </label>
                        <IconButton
                          icon="tree"
                          label={t(
                            "sqlPage.structure.documentReplaceWithExists",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null ||
                            node().kind === "other_expression"}
                          active={replaceWithExists()}
                          onClick={() => {
                            setReplaceWithExists(true);
                            setSubqueryPredicateTemplate("");
                          }}
                        />
                      </div>
                    </Show>
                    <Show when={isExpressionKind(node().kind)}>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        disabled={busy() || props.disabled?.() ||
                          node().range === null}
                        onClick={() => {
                          setCaseBranchAction("wrap");
                          setWrapCastTargetType("");
                          setWrapAggregateFunction("");
                          setExpressionLeafAction("");
                        }}
                      >
                        {t("sqlPage.structure.documentWrapExpressionInCase")}
                      </button>
                    </Show>

                    <Show
                      when={node().kind === "common_table_expression" &&
                        node().semantic?.kind === "common_table_expression"}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentCteName")}
                          <input
                            class="ui-input"
                            value={cteName().value}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onInput={(event) =>
                              setCteName((current) => ({
                                ...current,
                                value: event.currentTarget.value,
                              }))}
                          />
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentQuoteStyle", {
                            index: 1,
                          })}
                          <select
                            class="ui-input"
                            value={cteName().quote_style ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              node().range === null}
                            onChange={(event) =>
                              setCteName((current) => ({
                                ...current,
                                quote_style: event.currentTarget.value || null,
                              }))}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentUnquoted")}
                            </option>
                            <For
                              each={identifierQuoteStyles.filter((style) =>
                                style !== null
                              )}
                            >
                              {(style) => (
                                <option value={style}>
                                  {style === "[" ? "[]" : `${style}${style}`}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="trash"
                          label={t("sqlPage.structure.documentRemoveCte")}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onClick={() => setCteAction("remove")}
                        />
                        <label class="ui-label">
                          {t("sqlPage.structure.documentCteQuery")}
                          <select
                            class="ui-input"
                            value={cteQueryNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              queryNodeCandidates(node()).length === 0}
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              setCteQueryNodeId(
                                value ? Number(value) : undefined,
                              );
                              setCteAction(value ? "replace" : "");
                            }}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseQuery")}
                            </option>
                            <For each={queryNodeCandidates(node())}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <Show when={cteQueryNodeId() !== undefined}>
                          <IconButton
                            icon="edit"
                            label={t(
                              "sqlPage.structure.documentReplaceCteQuery",
                            )}
                            disabled={busy() || props.disabled?.() ||
                              !selectedCteQuery()}
                            onClick={() => setCteAction("replace")}
                          />
                        </Show>
                        <IconButton
                          icon="arrow-up"
                          label={t("sqlPage.structure.documentMoveCteEarlier")}
                          disabled={busy() || props.disabled?.() ||
                            cteSiblingIndex(node()) <= 0}
                          onClick={() => {
                            setCteMoveToIndex(cteSiblingIndex(node()) - 1);
                            setCteAction("move");
                          }}
                        />
                        <IconButton
                          icon="arrow-down"
                          label={t("sqlPage.structure.documentMoveCteLater")}
                          disabled={busy() || props.disabled?.() ||
                            cteSiblingIndex(node()) < 0 ||
                            cteSiblingIndex(node()) >=
                              cteSiblings(node()).length - 1}
                          onClick={() => {
                            setCteMoveToIndex(cteSiblingIndex(node()) + 1);
                            setCteAction("move");
                          }}
                        />
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "predicate_expression" &&
                        node().semantic?.kind === "predicate" &&
                        negatablePredicates.has(node().semantic.operator)}
                    >
                      <label class="ui-label sql-document-tree__negation">
                        <input
                          type="checkbox"
                          checked={predicateIsNegated()}
                          disabled={busy() || props.disabled?.()}
                          onChange={(event) =>
                            setPredicateIsNegated(event.currentTarget.checked)}
                        />
                        {t("sqlPage.structure.documentPredicateNegated")}
                      </label>
                    </Show>

                    <Show
                      when={node().kind === "predicate_expression" &&
                        node().semantic?.kind === "predicate" &&
                        node().semantic.operator === "in"}
                    >
                      <div class="sql-document-tree__fields">
                        <IconButton
                          icon="plus"
                          label={t(
                            "sqlPage.structure.documentAppendInListValue",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onClick={() => setInListAction("append")}
                        />
                        <IconButton
                          icon="minus"
                          label={t(
                            "sqlPage.structure.documentRemoveInListValue",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            node().range === null}
                          onClick={() => setInListAction("remove_last")}
                        />
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "case_expression" &&
                        node().semantic?.kind === "case"}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentCaseMode")}
                          <select
                            class="ui-input"
                            value={node().semantic?.kind === "case" && node().semantic.has_operand ? caseOperandNodeId() ?? "" : ""}
                            disabled={busy() || props.disabled?.() || node().range === null}
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              if (!value) {
                                setCaseOperandAction(node().semantic?.kind === "case" && node().semantic.has_operand ? "clear" : "");
                                return;
                              }
                              setCaseOperandNodeId(Number(value));
                              setCaseOperandAction("set");
                            }}
                          >
                            <option value="">{t("sqlPage.structure.documentSearchedCase")}</option>
                            <Show when={selectedCaseOperandNode()}>
                              {(operand) => <option value={operand().id}>{sourceSlice(document()?.source ?? "", operand())}</option>}
                            </Show>
                            <For each={queryExpressionCandidates()}>
                              {(candidate) => <option value={candidate.id}>{sourceSlice(document()?.source ?? "", candidate)}</option>}
                            </For>
                          </select>
                        </label>
                        <span>
                          {t("sqlPage.structure.documentCaseBranches", {
                            count: node().semantic?.kind === "case"
                              ? node().semantic.when_count
                              : 0,
                          })}
                        </span>
                        <div class="sql-document-tree__case-actions">
                          <IconButton
                            icon="minus"
                            label={t(
                              "sqlPage.structure.documentRemoveCaseBranch",
                            )}
                            disabled={busy() || props.disabled?.() ||
                              node().semantic?.kind !== "case" ||
                              node().semantic.when_count <= 1}
                            onClick={() => setCaseBranchAction("remove")}
                          />
                          <IconButton
                            icon="plus"
                            label={t(
                              "sqlPage.structure.documentAddCaseBranch",
                            )}
                            disabled={busy() || props.disabled?.() ||
                              node().semantic?.kind !== "case" ||
                              node().semantic.when_count >= 32}
                            onClick={() => setCaseBranchAction("add")}
                          />
                        </div>
                        <label class="ui-label sql-document-tree__negation">
                          <input
                            type="checkbox"
                            checked={caseHasElse()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) =>
                              setCaseHasElse(event.currentTarget.checked)}
                          />
                          {t("sqlPage.structure.documentCaseElse")}
                        </label>
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "select_item" ||
                        node().kind === "select_list"}
                    >
                      <label class="ui-label">
                        {t("sqlPage.structure.documentFunctionExpression")}
                        <select
                          class="ui-input"
                          value={projectionExpressionNodeId() ?? ""}
                          disabled={busy() || props.disabled?.()}
                          onChange={(event) => {
                            const value = event.currentTarget.value;
                            setProjectionExpressionNodeId(
                              value === "" ? undefined : Number(value),
                            );
                          }}
                        >
                          <option value="">
                            {t("sqlPage.structure.documentChooseExpression")}
                          </option>
                          <For each={queryExpressionCandidates()}>
                            {(candidate) => (
                              <option value={candidate.id}>
                                {sourceSlice(
                                  document()?.source ?? "",
                                  candidate,
                                )}
                              </option>
                            )}
                          </For>
                        </select>
                      </label>
                      <Show when={node().kind === "select_list"}>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentProjectionColumn")}
                          <select
                            class="ui-input"
                            value={columnChoice()}
                            disabled={busy() || props.disabled?.() ||
                              columnChoiceOptions(node()).length === 0}
                            onChange={(event) => {
                              setColumnChoice(event.currentTarget.value);
                              setAddProjectionColumn(
                                Boolean(event.currentTarget.value),
                              );
                            }}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseColumn")}
                            </option>
                            <For each={columnChoiceOptions(node())}>
                              {(option) => (
                                <option value={option.id}>
                                  {option.label} · {option.column}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="plus"
                          label={t(
                            "sqlPage.structure.documentAddProjectionColumn",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            !selectedColumnChoice()}
                          onClick={() => setAddProjectionColumn(true)}
                        />
                        <IconButton
                          icon="plus"
                          label={t(
                            "sqlPage.structure.documentAddScalarSubquery",
                          )}
                          disabled={busy() || props.disabled?.()}
                          onClick={() => setAddScalarSubquery(true)}
                        />
                      </Show>
                    </Show>

                    <Show when={node().kind === "select"}>
                      <div class="sql-document-tree__fields">
                        <Show when={node().semantic?.kind === "select"}>
                          <label class="ui-label sql-document-tree__negation">
                            <input
                              type="checkbox"
                              checked={selectDistinct()}
                              disabled={busy() || props.disabled?.() ||
                                node().semantic?.kind !== "select" ||
                                node().semantic?.distinct_on}
                              onChange={(event) =>
                                setSelectDistinct(event.currentTarget.checked)}
                            />
                            {t("sqlPage.structure.documentSelectDistinct")}
                          </label>
                        </Show>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentGroupByExpression")}
                          <select
                            class="ui-input"
                            value={projectionExpressionNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              queryExpressionCandidates().length === 0}
                            onChange={(event) =>
                              setProjectionExpressionNodeId(
                                event.currentTarget.value === ""
                                  ? undefined
                                  : Number(event.currentTarget.value),
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={queryExpressionCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="plus"
                          label={t(
                            "sqlPage.structure.documentAddGroupByExpression",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            !selectedProjectionExpression()}
                          onClick={() => setGroupByExpressionAction("add")}
                        />
                        <IconButton
                          icon="filter"
                          label={t("sqlPage.structure.documentAddWhereClause")}
                          disabled={busy() || props.disabled?.() ||
                            selectHasWhereClause(node())}
                          onClick={() => setAddWhereClause(true)}
                        />
                        <IconButton
                          icon="aggregate"
                          label={t("sqlPage.structure.documentAddHavingClause")}
                          disabled={busy() || props.disabled?.() ||
                            selectHasHavingClause(node())}
                          onClick={() => setAddHavingClause(true)}
                        />
                      </div>
                    </Show>

                    <Show when={node().kind === "group_by_item"}>
                      <div class="sql-document-tree__case-actions">
                        <IconButton
                          icon="arrow-up"
                          label={t(
                            "sqlPage.structure.documentMoveGroupByEarlier",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            listItemIndex(node()) <= 0}
                          onClick={() => {
                            setGroupByItemAction("move");
                            setGroupByMoveToIndex(listItemIndex(node()) - 1);
                          }}
                        />
                        <IconButton
                          icon="arrow-down"
                          label={t(
                            "sqlPage.structure.documentMoveGroupByLater",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            listItemIndex(node()) < 0 ||
                            listItemIndex(node()) >=
                              listItems(node(), "group_by_item").length - 1}
                          onClick={() => {
                            setGroupByItemAction("move");
                            setGroupByMoveToIndex(listItemIndex(node()) + 1);
                          }}
                        />
                        <IconButton
                          icon="trash"
                          label={t(
                            "sqlPage.structure.documentRemoveGroupByItem",
                          )}
                          disabled={busy() || props.disabled?.()}
                          onClick={() => setGroupByItemAction("remove")}
                        />
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "where_clause" ||
                        node().kind === "having_clause"}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentFilterExpression")}
                          <select
                            class="ui-input"
                            value={filterExpressionNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              filterExpressionCandidates(node()).length === 0}
                            onChange={(event) => {
                              setFilterExpressionNodeId(
                                event.currentTarget.value
                                  ? Number(event.currentTarget.value)
                                  : undefined,
                              );
                              setFilterAction(
                                event.currentTarget.value ? "set" : "",
                              );
                            }}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={filterExpressionCandidates(node())}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="edit"
                          label={t(
                            "sqlPage.structure.documentSetFilterExpression",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            !selectedFilterExpression()}
                          onClick={() => setFilterAction("set")}
                        />
                        <IconButton
                          icon="trash"
                          label={t(
                            "sqlPage.structure.documentRemoveFilterClause",
                          )}
                          disabled={busy() || props.disabled?.()}
                          onClick={() => setFilterAction("remove")}
                        />
                      </div>
                    </Show>

                    <Show when={node().kind === "query"}>
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentLimitValue")}
                          <input
                            class="ui-input"
                            inputMode="numeric"
                            value={limitValue()}
                            disabled={busy() || props.disabled?.()}
                            onInput={(event) => {
                              setLimitValue(event.currentTarget.value);
                              setQueryClauseAction("set_limit");
                            }}
                          />
                        </label>
                        <IconButton
                          icon="row-limit"
                          label={t("sqlPage.structure.documentSetLimit")}
                          disabled={busy() || props.disabled?.() ||
                            unsignedDraft(limitValue()) === undefined}
                          onClick={() => setQueryClauseAction("set_limit")}
                        />
                        <label class="ui-label">
                          {t("sqlPage.structure.documentOffsetValue")}
                          <input
                            class="ui-input"
                            inputMode="numeric"
                            value={offsetValue()}
                            disabled={busy() || props.disabled?.()}
                            onInput={(event) => {
                              setOffsetValue(event.currentTarget.value);
                              setQueryClauseAction("set_offset");
                            }}
                          />
                        </label>
                        <IconButton
                          icon="row-limit"
                          label={t("sqlPage.structure.documentSetOffset")}
                          disabled={busy() || props.disabled?.() ||
                            unsignedDraft(offsetValue()) === undefined}
                          onClick={() => setQueryClauseAction("set_offset")}
                        />
                        <IconButton
                          icon="trash"
                          label={t("sqlPage.structure.documentRemoveLimit")}
                          disabled={busy() || props.disabled?.() ||
                            !queryHasClause(node(), "limit_clause")}
                          onClick={() => setQueryClauseAction("remove_limit")}
                        />
                        <IconButton
                          icon="trash"
                          label={t("sqlPage.structure.documentRemoveOffset")}
                          disabled={busy() || props.disabled?.() ||
                            !queryHasClause(node(), "offset_clause")}
                          onClick={() => setQueryClauseAction("remove_offset")}
                        />
                        <label class="ui-label">
                          {t("sqlPage.structure.documentOrderByExpression")}
                          <select
                            class="ui-input"
                            value={projectionExpressionNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              queryExpressionCandidates().length === 0}
                            onChange={(event) =>
                              setProjectionExpressionNodeId(
                                event.currentTarget.value === ""
                                  ? undefined
                                  : Number(event.currentTarget.value),
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={queryExpressionCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="plus"
                          label={t(
                            "sqlPage.structure.documentAddOrderByExpression",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            !selectedProjectionExpression()}
                          onClick={() => setOrderByExpressionAction("add")}
                        />
                      </div>
                    </Show>

                    <Show when={node().kind === "order_by_item"}>
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentOrderByExpression")}
                          <select
                            class="ui-input"
                            value={projectionExpressionNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              queryExpressionCandidates().length === 0}
                            onChange={(event) =>
                              setProjectionExpressionNodeId(
                                event.currentTarget.value
                                  ? Number(event.currentTarget.value)
                                  : undefined,
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={queryExpressionCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentOrderDirection")}
                          <select
                            class="ui-input"
                            value={orderDirection()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) =>
                              setOrderDirection(
                                event.currentTarget.value as OrderDirection,
                              )}
                          >
                            <option value="asc">ASC</option>
                            <option value="desc">DESC</option>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentNullsOrder")}
                          <select
                            class="ui-input"
                            value={nullsOrder()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) =>
                              setNullsOrder(
                                event.currentTarget.value as NullsOrder,
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentNullsDefault")}
                            </option>
                            <option value="first">NULLS FIRST</option>
                            <option value="last">NULLS LAST</option>
                          </select>
                        </label>
                        <IconButton
                          icon="arrow-up"
                          label={t(
                            "sqlPage.structure.documentMoveOrderEarlier",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            listItemIndex(node()) <= 0}
                          onClick={() => {
                            setOrderByItemAction("move");
                            setOrderByMoveToIndex(listItemIndex(node()) - 1);
                          }}
                        />
                        <IconButton
                          icon="arrow-down"
                          label={t("sqlPage.structure.documentMoveOrderLater")}
                          disabled={busy() || props.disabled?.() ||
                            listItemIndex(node()) < 0 ||
                            listItemIndex(node()) >=
                              listItems(node(), "order_by_item").length - 1}
                          onClick={() => {
                            setOrderByItemAction("move");
                            setOrderByMoveToIndex(listItemIndex(node()) + 1);
                          }}
                        />
                        <IconButton
                          icon="trash"
                          label={t("sqlPage.structure.documentRemoveOrderItem")}
                          disabled={busy() || props.disabled?.()}
                          onClick={() => setOrderByItemAction("remove")}
                        />
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "limit_clause" ||
                        node().kind === "offset_clause"}
                    >
                      <div class="sql-document-tree__fields">
                        <Show when={node().kind === "limit_clause"}>
                          <label class="ui-label">
                            {t("sqlPage.structure.documentLimitValue")}
                            <input
                              class="ui-input"
                              inputMode="numeric"
                              value={limitValue()}
                              disabled={busy() || props.disabled?.()}
                              onInput={(event) => {
                                setLimitValue(event.currentTarget.value);
                                setQueryClauseAction("set_limit");
                              }}
                            />
                          </label>
                          <IconButton
                            icon="edit"
                            label={t("sqlPage.structure.documentSetLimit")}
                            disabled={busy() || props.disabled?.() ||
                              unsignedDraft(limitValue()) === undefined}
                            onClick={() => setQueryClauseAction("set_limit")}
                          />
                        </Show>
                        <Show when={node().kind === "offset_clause"}>
                          <label class="ui-label">
                            {t("sqlPage.structure.documentOffsetValue")}
                            <input
                              class="ui-input"
                              inputMode="numeric"
                              value={offsetValue()}
                              disabled={busy() || props.disabled?.()}
                              onInput={(event) => {
                                setOffsetValue(event.currentTarget.value);
                                setQueryClauseAction("set_offset");
                              }}
                            />
                          </label>
                          <IconButton
                            icon="edit"
                            label={t("sqlPage.structure.documentSetOffset")}
                            disabled={busy() || props.disabled?.() ||
                              unsignedDraft(offsetValue()) === undefined}
                            onClick={() => setQueryClauseAction("set_offset")}
                          />
                        </Show>
                        <IconButton
                          icon="trash"
                          label={t("sqlPage.structure.documentRemoveLimit")}
                          disabled={busy() || props.disabled?.() ||
                            !queryHasClause(
                              queryAncestor(node()) ?? node(),
                              "limit_clause",
                            )}
                          onClick={() => setQueryClauseAction("remove_limit")}
                        />
                        <IconButton
                          icon="trash"
                          label={t("sqlPage.structure.documentRemoveOffset")}
                          disabled={busy() || props.disabled?.() ||
                            !queryHasClause(
                              queryAncestor(node()) ?? node(),
                              "offset_clause",
                            )}
                          onClick={() => setQueryClauseAction("remove_offset")}
                        />
                      </div>
                    </Show>

                    <Show when={node().kind === "select_item"}>
                      <label class="ui-field sql-document-tree__fields">
                        <span>{t("sqlPage.columns.aliasLabel")}</span>
                        <input
                          class="ui-input sql-columns-editor__alias"
                          value={node().semantic?.kind === "projection_alias"
                            ? node().semantic.alias.value
                            : ""}
                          placeholder={t(
                            "sqlPage.columns.outputAliasPlaceholder",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            node().semantic?.kind === "wildcard"}
                          onBlur={(event) => {
                            const current =
                              node().semantic?.kind === "projection_alias"
                                ? node().semantic.alias.value
                                : "";
                            const value = event.currentTarget.value.trim();
                            if (current === value) return;
                            void applyEdit({
                              kind: "set_projection_alias",
                              node_id: node().id,
                              expected_kind: "select_item",
                              alias: value ? { value, quote_style: '"' } : null,
                            });
                          }}
                        />
                      </label>
                      <Show when={node().semantic?.kind === "wildcard"}>
                        <IconButton
                          icon="columns"
                          label={t("sqlPage.structure.documentExpandWildcard")}
                          disabled={busy() || props.disabled?.() ||
                            !wildcardQualifier(node())}
                          onClick={() => setExpandWildcard(true)}
                        />
                      </Show>
                      <div class="sql-document-tree__case-actions">
                        <IconButton
                          icon="arrow-up"
                          label={t(
                            "sqlPage.structure.documentMoveProjectionUp",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            selectedProjectionItems().length < 2 ||
                            selectedProjectionItemIndex() <= 0}
                          onClick={() => {
                            setProjectionItemAction("move");
                            setProjectionMoveToIndex(
                              selectedProjectionItemIndex() - 1,
                            );
                          }}
                        />
                        <IconButton
                          icon="arrow-down"
                          label={t(
                            "sqlPage.structure.documentMoveProjectionDown",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            selectedProjectionItems().length < 2 ||
                            selectedProjectionItemIndex() < 0 ||
                            selectedProjectionItemIndex() >=
                              selectedProjectionItems().length - 1}
                          onClick={() => {
                            setProjectionItemAction("move");
                            setProjectionMoveToIndex(
                              selectedProjectionItemIndex() + 1,
                            );
                          }}
                        />
                        <IconButton
                          icon="trash"
                          label={t(
                            "sqlPage.structure.documentRemoveProjection",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            selectedProjectionItems().length < 2}
                          onClick={() => {
                            setProjectionItemAction("remove");
                            setProjectionMoveToIndex(undefined);
                          }}
                        />
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "select_item" ||
                        isExpressionKind(node().kind)}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentLeftExpression")}
                          <select
                            class="ui-input"
                            value={binaryExpressionLeftNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              queryExpressionCandidates().length === 0}
                            onChange={(event) =>
                              setBinaryExpressionLeftNodeId(
                                event.currentTarget.value === ""
                                  ? undefined
                                  : Number(event.currentTarget.value),
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={queryExpressionCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentBinaryOperator")}
                          <select
                            class="ui-input"
                            value={binaryExpressionOperator()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) =>
                              setBinaryExpressionOperator(
                                event.currentTarget
                                  .value as
                                    | SqlDocumentSemanticBinaryOperator
                                    | "",
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseOperator")}
                            </option>
                            <For each={binaryOperators}>
                              {(item) => (
                                <option value={item.kind}>{item.label}</option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentRightExpression")}
                          <select
                            class="ui-input"
                            value={binaryExpressionRightNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              queryExpressionCandidates().length === 0}
                            onChange={(event) =>
                              setBinaryExpressionRightNodeId(
                                event.currentTarget.value === ""
                                  ? undefined
                                  : Number(event.currentTarget.value),
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={queryExpressionCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="plus"
                          label={t(
                            "sqlPage.structure.documentBuildBinaryExpression",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            !selectedBinaryExpressionLeftNode() ||
                            !binaryExpressionOperator() ||
                            !selectedBinaryExpressionRightNode()}
                          onClick={() => {
                            setExpressionLeafAction("");
                            setBinaryExpressionAction("compose");
                          }}
                        />
                      </div>
                    </Show>

                    <Show when={isExpressionKind(node().kind)}>
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentChooseExpression")}
                          <select
                            class="ui-input"
                            value={projectionExpressionNodeId() ?? ""}
                            disabled={busy() || props.disabled?.() ||
                              queryExpressionCandidates().length === 0}
                            onChange={(event) =>
                              setProjectionExpressionNodeId(
                                event.currentTarget.value === ""
                                  ? undefined
                                  : Number(event.currentTarget.value),
                              )}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={queryExpressionCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="edit"
                          label={t(
                            "sqlPage.structure.documentReplaceExpression",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            !selectedProjectionExpression() ||
                            selectedProjectionExpression()?.id === node().id}
                          onClick={() => {
                            setExpressionLeafAction("");
                            setBinaryExpressionAction("replace");
                          }}
                        />
                      </div>
                    </Show>

                    <Show
                      when={node().kind === "function_expression" &&
                        Boolean(aggregateFunctionKind(node()))}
                    >
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentAggregateFunction")}
                          <select
                            class="ui-input"
                            value={aggregateFunction()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) => {
                              setAggregateFunction(
                                event.currentTarget.value as
                                  | SqlDocumentAggregateFunction
                                  | "",
                              );
                              setAggregateAction("set_function");
                            }}
                          >
                            <For each={aggregateFunctions}>
                              {(item) => (
                                <option value={item.kind}>{item.label}</option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label class="ui-label sql-document-tree__negation">
                          <input
                            type="checkbox"
                            checked={aggregateDistinct()}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) => {
                              setAggregateDistinct(event.currentTarget.checked);
                              setAggregateAction("set_distinct");
                            }}
                          />
                          {t("sqlPage.structure.documentAggregateDistinct")}
                        </label>
                        <IconButton
                          icon="trash"
                          label={t("sqlPage.structure.documentRemoveAggregate")}
                          disabled={busy() || props.disabled?.()}
                          onClick={() => setAggregateAction("remove")}
                        />
                      </div>
                    </Show>

                    <Show when={hasFunctionArgumentInspector(node())}>
                      <div class="sql-document-tree__fields">
                        <label class="ui-label">
                          {t("sqlPage.structure.documentFunctionArgument")}
                          <select
                            class="ui-input"
                            value={functionArgumentIndex()}
                            disabled={busy() || props.disabled?.() ||
                              !functionExpressionNodeId()}
                            onChange={(event) => {
                              setFunctionArgumentIndex(
                                Number(event.currentTarget.value),
                              );
                              setFunctionArgumentAction("set");
                            }}
                          >
                            <Show
                              when={node().semantic?.kind === "function" &&
                                (node().semantic.argument_count ?? 0) > 0}
                            >
                              <For
                                each={Array.from({
                                  length: node().semantic?.kind === "function"
                                    ? node().semantic.argument_count ?? 0
                                    : 0,
                                }, (_, index) => index)}
                              >
                                {(index) => (
                                  <option value={index}>
                                    {t(
                                      "sqlPage.structure.documentFunctionArgumentIndex",
                                      { index: index + 1 },
                                    )}
                                  </option>
                                )}
                              </For>
                            </Show>
                            <Show
                              when={node().semantic?.kind === "function" &&
                                node().semantic.can_add_argument}
                            >
                              <option
                                value={node().semantic?.kind === "function"
                                  ? node().semantic.argument_count ?? 0
                                  : 0}
                              >
                                {t(
                                  "sqlPage.structure.documentFunctionAppendArgument",
                                )}
                              </option>
                            </Show>
                          </select>
                        </label>
                        <label class="ui-label">
                          {t("sqlPage.structure.documentFunctionExpression")}
                          <select
                            class="ui-input"
                            value={functionExpressionNodeId() ?? ""}
                            disabled={busy() || props.disabled?.()}
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              setFunctionExpressionNodeId(
                                value === "" ? undefined : Number(value),
                              );
                              setFunctionArgumentAction(
                                value === "" ? "" : "set",
                              );
                            }}
                          >
                            <option value="">
                              {t("sqlPage.structure.documentChooseExpression")}
                            </option>
                            <For each={functionArgumentCandidates()}>
                              {(candidate) => (
                                <option value={candidate.id}>
                                  {sourceSlice(
                                    document()?.source ?? "",
                                    candidate,
                                  )}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <IconButton
                          icon="minus"
                          label={t(
                            "sqlPage.structure.documentRemoveFunctionArgument",
                          )}
                          disabled={busy() || props.disabled?.() ||
                            node().semantic?.kind !== "function" ||
                            !node().semantic.can_remove_argument}
                          onClick={() => setFunctionArgumentAction("remove")}
                        />
                        <Show when={functionArgumentCandidates().length === 0}>
                          <span class="ui-text-muted" role="status">
                            {t(
                              "sqlPage.structure.documentNoFunctionExpressions",
                            )}
                          </span>
                        </Show>
                      </div>
                    </Show>

                    <Show
                      when={!currentSemanticEdit() &&
                        node().kind !== "case_expression" &&
                        node().kind !== "table_factor" &&
                        node().kind !== "relation_tree" &&
                        node().kind !== "join" &&
                        !hasFunctionArgumentInspector(node()) &&
                        !(node().kind === "set_operation" &&
                          (setOperationOperator() === "" ||
                            setOperationQuantifier() === ""))}
                    >
                      <span class="ui-text-muted">
                        {t("sqlPage.structure.documentNoEdit")}
                      </span>
                    </Show>
                    <button
                      type="button"
                      class="btn primary"
                      disabled={!editorCanApply()}
                      onClick={() => void apply()}
                    >
                      <Show
                        when={busy()}
                        fallback={t("sqlPage.structure.documentApply")}
                      >
                        {t("common.saving")}
                      </Show>
                    </button>
                  </section>
                )}
              </Show>
            </div>
            <Show when={error()}>
              <p class="ui-text-danger" role="alert">{error()}</p>
            </Show>
          </>
        )}
      </Show>
    </div>
  );
}
