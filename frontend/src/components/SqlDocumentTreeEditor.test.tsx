import "@testing-library/jest-dom/vitest";
import { createSignal } from "solid-js";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SqlDocument } from "~/lib/ugoite-client";
import { SqlDocumentTreeEditor } from "./SqlDocumentTreeEditor";

const { applySemanticEditMock, parseDocumentMock } = vi.hoisted(() => ({
  applySemanticEditMock: vi.fn(),
  parseDocumentMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  applySqlDocumentSemanticEditTransaction: applySemanticEditMock,
  parseSqlDocument: parseDocumentMock,
}));

const makeDocument = (
  source: string,
  operator: string,
  quantifier: string,
): SqlDocument => ({
  contract_version: 14,
  source,
  status: "parsed",
  diagnostics: [],
  tokens: [],
  nodes: [
    {
      id: 0,
      parent: null,
      kind: "query_statement",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 1,
      parent: 0,
      kind: "set_operation",
      range: { start: 0, end: source.length },
      semantic: { kind: "set_operation", operator, quantifier },
    },
    {
      id: 2,
      parent: 1,
      kind: "set_operation_left",
      range: { start: 0, end: 8 },
      semantic: null,
    },
    {
      id: 3,
      parent: 1,
      kind: "set_operation_right",
      range: { start: source.lastIndexOf("SELECT"), end: source.length },
      semantic: null,
    },
  ],
});

const makeCastDocument = (
  source: string,
  targetType: string,
  castKind = "cast",
): SqlDocument => {
  const tryCastStart = source.indexOf("TRY_CAST(");
  const castStart = tryCastStart >= 0 ? tryCastStart : source.indexOf("CAST(");
  const operandStart = source.indexOf("amount", castStart);
  const operandEnd = operandStart + "amount".length;
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "cast_expression",
        range: { start: castStart, end: source.length },
        semantic: {
          kind: "cast",
          cast_kind: castKind,
          target_type: targetType,
        },
      },
      {
        id: 2,
        parent: 1,
        kind: "identifier_expression",
        range: { start: operandStart, end: operandEnd },
        semantic: {
          kind: "identifier",
          value: "amount",
          quote_style: null,
        },
      },
    ],
  };
};

const makeCteDocument = (
  source: string,
  name: string,
  quoteStyle: string | null,
): SqlDocument => {
  const cteStart = source.indexOf("WITH") + "WITH ".length;
  const cteEnd = source.indexOf(")", cteStart) + 1;
  const relationStart = source.lastIndexOf(name);
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "with_clause",
        range: { start: 0, end: cteEnd },
        semantic: { kind: "with_clause", recursive: false, cte_count: 1 },
      },
      {
        id: 2,
        parent: 1,
        kind: "common_table_expression",
        range: { start: cteStart, end: cteEnd },
        semantic: {
          kind: "common_table_expression",
          name: { value: name, quote_style: quoteStyle },
          columns: [],
          materialized: null,
        },
      },
      {
        id: 3,
        parent: 0,
        kind: "relation",
        range: { start: relationStart, end: relationStart + name.length },
        semantic: {
          kind: "relation",
          name: [{ value: name, quote_style: quoteStyle }],
        },
      },
    ],
  };
};

const makeSelectDocument = (source: string): SqlDocument => ({
  contract_version: 14,
  source,
  status: "parsed",
  diagnostics: [],
  tokens: [],
  nodes: [
    {
      id: 0,
      parent: null,
      kind: "query_statement",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 1,
      parent: 0,
      kind: "query",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 2,
      parent: 1,
      kind: "select",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    ...(source.includes("WHERE")
      ? [{
        id: 3,
        parent: 2,
        kind: "where_clause" as const,
        range: { start: source.indexOf("WHERE"), end: source.length },
        semantic: null,
      }]
      : []),
  ],
});

const makeRelationDocument = (source: string): SqlDocument => {
  const selectStart = source.indexOf("SELECT ") + "SELECT ".length;
  const fromStart = source.indexOf("FROM");
  const relationStart = source.indexOf('"G"');
  const relationEnd = relationStart + '"G"'.length;
  const relationTreeEnd = source.includes(" WHERE ")
    ? source.indexOf(" WHERE ")
    : source.length;
  const whereStart = source.indexOf(" WHERE ");
  const conditionStart = whereStart < 0 ? -1 : whereStart + " WHERE ".length;
  const conditionEnd = conditionStart < 0 ? -1 : source.length;
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "query",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 2,
        parent: 1,
        kind: "select",
        range: { start: 0, end: relationTreeEnd },
        semantic: null,
      },
      {
        id: 3,
        parent: 2,
        kind: "select_list",
        range: { start: selectStart, end: fromStart - 1 },
        semantic: null,
      },
      {
        id: 4,
        parent: 3,
        kind: "select_item",
        range: { start: selectStart, end: fromStart - 1 },
        semantic: null,
      },
      {
        id: 5,
        parent: 4,
        kind: "compound_identifier_expression",
        range: { start: selectStart, end: fromStart - 1 },
        semantic: {
          kind: "compound_identifier",
          parts: [
            { value: "g", quote_style: null },
            { value: "id", quote_style: null },
          ],
        },
      },
      {
        id: 6,
        parent: 2,
        kind: "from_clause",
        range: { start: fromStart, end: relationTreeEnd },
        semantic: null,
      },
      {
        id: 7,
        parent: 6,
        kind: "relation_tree",
        range: { start: relationStart, end: relationTreeEnd },
        semantic: null,
      },
      {
        id: 8,
        parent: 7,
        kind: "table_factor",
        range: { start: relationStart, end: relationEnd + " AS g".length },
        semantic: {
          kind: "table_factor",
          factor_kind: "table",
          alias: { value: "g", quote_style: null },
        },
      },
      {
        id: 9,
        parent: 8,
        kind: "relation",
        range: { start: relationStart, end: relationEnd },
        semantic: {
          kind: "relation",
          name: [{ value: "G", quote_style: '"' }],
        },
      },
      ...(whereStart >= 0
        ? [{
          id: 10,
          parent: 2,
          kind: "where_clause" as const,
          range: { start: whereStart, end: source.length },
          semantic: null,
        }, {
          id: 11,
          parent: 10,
          kind: "binary_expression" as const,
          range: { start: conditionStart, end: conditionEnd },
          semantic: { kind: "binary_operator" as const, operator: "=" },
        }, {
          id: 12,
          parent: 11,
          kind: "compound_identifier_expression" as const,
          range: {
            start: conditionStart,
            end: conditionStart + "g.id".length,
          },
          semantic: {
            kind: "compound_identifier" as const,
            parts: [
              { value: "g", quote_style: null },
              { value: "id", quote_style: null },
            ],
          },
        }, {
          id: 13,
          parent: 11,
          kind: "value" as const,
          range: {
            start: conditionStart + "g.id = ".length,
            end: conditionEnd,
          },
          semantic: {
            kind: "literal" as const,
            literal_kind: "number" as const,
            raw: "1",
          },
        }]
        : []),
    ],
  };
};

const makeScalarProjectionListDocument = (source: string): SqlDocument => {
  const listStart = source.indexOf("SELECT ") + "SELECT ".length;
  const listEnd = source.indexOf(" FROM", listStart);
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "query",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 2,
        parent: 1,
        kind: "select",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 3,
        parent: 2,
        kind: "select_list",
        range: { start: listStart, end: listEnd },
        semantic: null,
      },
      {
        id: 4,
        parent: 3,
        kind: "select_item",
        range: { start: listStart, end: listEnd },
        semantic: null,
      },
      {
        id: 5,
        parent: 4,
        kind: "literal_expression",
        range: { start: listStart, end: listEnd },
        semantic: { kind: "literal", literal_kind: "number", raw: "1" },
      },
    ],
  };
};

const makeQueryDocument = (source: string): SqlDocument => ({
  contract_version: 14,
  source,
  status: "parsed",
  diagnostics: [],
  tokens: [],
  nodes: [
    {
      id: 0,
      parent: null,
      kind: "query_statement",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 1,
      parent: 0,
      kind: "query",
      range: { start: 0, end: source.length },
      semantic: null,
    },
  ],
});

const makeSetOperationTreeDocument = (
  source: string,
  operator = "UNION",
  quantifier = "ALL",
): SqlDocument => {
  const operatorStart = source.indexOf(operator);
  const leftStart = source.indexOf("SELECT");
  const rightStart = source.lastIndexOf("SELECT");
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "set_operation",
        range: { start: 0, end: source.length },
        semantic: { kind: "set_operation", operator, quantifier },
      },
      {
        id: 2,
        parent: 1,
        kind: "set_operation_left",
        range: { start: leftStart, end: operatorStart },
        semantic: null,
      },
      {
        id: 3,
        parent: 2,
        kind: "query",
        range: { start: leftStart, end: operatorStart },
        semantic: null,
      },
      {
        id: 4,
        parent: 3,
        kind: "select",
        range: { start: leftStart, end: operatorStart },
        semantic: null,
      },
      {
        id: 5,
        parent: 1,
        kind: "set_operation_right",
        range: { start: rightStart, end: source.length },
        semantic: null,
      },
      {
        id: 6,
        parent: 5,
        kind: "query",
        range: { start: rightStart, end: source.length },
        semantic: null,
      },
      {
        id: 7,
        parent: 6,
        kind: "select",
        range: { start: rightStart, end: source.length },
        semantic: null,
      },
    ],
  };
};

const makeTwoCteDocument = (source: string): SqlDocument => {
  const firstStart = source.indexOf("rows AS");
  const secondStart = source.indexOf("other AS");
  const firstBodyStart = source.indexOf("SELECT 1", firstStart);
  const firstBodyEnd = firstBodyStart + "SELECT 1".length;
  const firstEnd = source.indexOf(")", firstBodyStart) + 1;
  const secondBodyStart = source.indexOf("SELECT 2", secondStart);
  const secondBodyEnd = secondBodyStart + "SELECT 2".length;
  const secondEnd = source.indexOf(")", secondBodyStart) + 1;
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "query",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 2,
        parent: 1,
        kind: "with_clause",
        range: { start: 0, end: secondEnd },
        semantic: { kind: "with_clause", recursive: false, cte_count: 2 },
      },
      {
        id: 3,
        parent: 2,
        kind: "common_table_expression",
        range: { start: firstStart, end: firstEnd },
        semantic: {
          kind: "common_table_expression",
          name: { value: "rows", quote_style: null },
          columns: [],
          materialized: null,
        },
      },
      {
        id: 4,
        parent: 3,
        kind: "query",
        range: { start: firstBodyStart, end: firstBodyEnd },
        semantic: null,
      },
      {
        id: 5,
        parent: 2,
        kind: "common_table_expression",
        range: { start: secondStart, end: secondEnd },
        semantic: {
          kind: "common_table_expression",
          name: { value: "other", quote_style: null },
          columns: [],
          materialized: null,
        },
      },
      {
        id: 6,
        parent: 5,
        kind: "query",
        range: { start: secondBodyStart, end: secondBodyEnd },
        semantic: null,
      },
      {
        id: 7,
        parent: 1,
        kind: "select",
        range: {
          start: source.lastIndexOf("SELECT 3"),
          end: source.length,
        },
        semantic: null,
      },
    ],
  };
};

const makePredicateDocument = (
  source: string,
  operator: string,
  negated: boolean,
): SqlDocument => {
  const operandStart = source.indexOf("amount", source.indexOf("WHERE"));
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "predicate_expression",
        range: { start: operandStart, end: source.length },
        semantic: { kind: "predicate", operator, negated },
      },
      {
        id: 2,
        parent: 1,
        kind: "identifier_expression",
        range: { start: operandStart, end: operandStart + "amount".length },
        semantic: {
          kind: "identifier",
          value: "amount",
          quote_style: null,
        },
      },
    ],
  };
};

const makeUnaryDocument = (source: string, operator: string): SqlDocument => {
  const operatorStart = source.indexOf(operator);
  const operandStart = source.indexOf("amount");
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "unary_expression",
        range: { start: operatorStart, end: source.length },
        semantic: { kind: "unary_operator", operator },
      },
      {
        id: 2,
        parent: 1,
        kind: "identifier_expression",
        range: { start: operandStart, end: operandStart + "amount".length },
        semantic: {
          kind: "identifier",
          value: "amount",
          quote_style: null,
        },
      },
    ],
  };
};

const makeCaseDocument = (source: string): SqlDocument => {
  const caseStart = source.indexOf("CASE");
  const caseEnd = source.indexOf("END", caseStart) + "END".length;
  const sumStart = source.indexOf("SUM(");
  const whenStart = source.indexOf("WHEN", caseStart) + "WHEN".length;
  const thenStart = source.indexOf("THEN", whenStart);
  const fieldStart = source.indexOf("field_100", whenStart);
  const stringStart = source.indexOf("'not-a-number'", fieldStart);
  const trueValueStart = source.indexOf("10", thenStart);
  const elseStart = source.indexOf("ELSE", caseStart);
  const falseValueStart = source.indexOf("1", elseStart);
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "function_expression",
        range: { start: sumStart, end: caseEnd + 1 },
        semantic: {
          kind: "function",
          name: [{ value: "SUM", quote_style: null }],
          argument_count: 1,
          distinct: false,
          has_filter: false,
          has_over: false,
          has_argument_clauses: false,
          can_replace_argument: true,
          can_add_argument: false,
          can_remove_argument: false,
        },
      },
      {
        id: 2,
        parent: 1,
        kind: "case_expression",
        range: { start: caseStart, end: caseEnd },
        semantic: {
          kind: "case",
          has_operand: false,
          when_count: (source.match(/\bWHEN\b/g) ?? []).length,
          has_else: /\bELSE\b/.test(source.slice(caseStart, caseEnd)),
        },
      },
      {
        id: 3,
        parent: 2,
        kind: "binary_expression",
        range: { start: whenStart, end: thenStart },
        semantic: { kind: "binary_operator", operator: "=" },
      },
      {
        id: 4,
        parent: 3,
        kind: "identifier_expression",
        range: { start: fieldStart, end: fieldStart + "field_100".length },
        semantic: {
          kind: "identifier",
          value: "field_100",
          quote_style: null,
        },
      },
      {
        id: 5,
        parent: 3,
        kind: "literal_expression",
        range: {
          start: stringStart,
          end: stringStart + "'not-a-number'".length,
        },
        semantic: {
          kind: "literal",
          literal_kind: "string",
          raw: "'not-a-number'",
        },
      },
      {
        id: 6,
        parent: 2,
        kind: "value",
        range: { start: trueValueStart, end: trueValueStart + 2 },
        semantic: { kind: "literal", literal_kind: "number", raw: "10" },
      },
      {
        id: 7,
        parent: 2,
        kind: "value",
        range: { start: falseValueStart, end: falseValueStart + 1 },
        semantic: { kind: "literal", literal_kind: "number", raw: "1" },
      },
    ],
  };
};

const makeCountDocument = (source: string): SqlDocument => {
  const functionStart = source.indexOf("COUNT(");
  const open = source.indexOf("(", functionStart);
  const close = source.indexOf(")", open);
  const argumentText = source.slice(open + 1, close);
  const argumentRanges = argumentText.trim()
    ? [...argumentText.matchAll(/amount/g)].map((match) => ({
      start: open + 1 + (match.index ?? 0),
      end: open + 1 + (match.index ?? 0) + "amount".length,
    }))
    : [];
  const projectionStart = source.lastIndexOf("amount");
  const projectionRange = projectionStart > close
    ? { start: projectionStart, end: projectionStart + "amount".length }
    : undefined;
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "function_expression",
        range: { start: functionStart, end: close + 1 },
        semantic: {
          kind: "function",
          name: [{ value: "COUNT", quote_style: null }],
          argument_count: argumentRanges.length,
          distinct: false,
          has_filter: false,
          has_over: false,
          has_argument_clauses: false,
          can_replace_argument: argumentRanges.length > 0,
          can_add_argument: argumentRanges.length < 32,
          can_remove_argument: argumentRanges.length > 0,
        },
      },
      ...argumentRanges.map((range, index) => ({
        id: index + 2,
        parent: 1,
        kind: "identifier_expression" as const,
        range,
        semantic: {
          kind: "identifier" as const,
          value: "amount",
          quote_style: null,
        },
      })),
      ...(projectionRange
        ? [{
          id: argumentRanges.length + 2,
          parent: 0,
          kind: "identifier_expression" as const,
          range: projectionRange,
          semantic: {
            kind: "identifier" as const,
            value: "amount",
            quote_style: null,
          },
        }]
        : []),
    ],
  };
};

const makeProjectionDocument = (source: string): SqlDocument => {
  const listStart = source.indexOf("SELECT ") + "SELECT ".length;
  const from = source.indexOf(" FROM ", listStart);
  const projectionText = source.slice(listStart, from);
  const projectionTexts = projectionText.split(",").map((text) => text.trim());
  const expressionNode = (
    id: number,
    parent: number,
    text: string,
    start: number,
  ): SqlDocumentNode =>
    text === "amount"
      ? {
        id,
        parent,
        kind: "identifier_expression",
        range: { start, end: start + text.length },
        semantic: { kind: "identifier", value: "amount", quote_style: null },
      }
      : {
        id,
        parent,
        kind: "binary_expression",
        range: { start, end: start + text.length },
        semantic: { kind: "binary_operator", operator: "+" },
      };
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "select_list",
        range: { start: 0, end: from },
        semantic: null,
      },
      ...projectionTexts.flatMap((text, index) => {
        let start = listStart;
        for (let prior = 0; prior < index; prior += 1) {
          start = source.indexOf(projectionTexts[prior], start) +
            projectionTexts[prior].length + 1;
        }
        start = source.indexOf(text, start);
        const end = start + text.length;
        const itemId = 2 + index * 2;
        return [
          {
            id: itemId,
            parent: 1,
            kind: "select_item" as const,
            range: { start, end },
            semantic: null,
          },
          expressionNode(itemId + 1, itemId, text, start),
        ];
      }),
    ],
  };
};

const makeBinaryProjectionDocument = (source: string): SqlDocument => {
  const first = source.indexOf("field_100");
  const secondLiteral = source.indexOf("'x'");
  const second = secondLiteral >= 0
    ? secondLiteral
    : source.indexOf("field_100", first + "field_100".length);
  const secondText = secondLiteral >= 0 ? "'x'" : "field_100";
  const selectEnd = source.indexOf(" FROM ");
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "query",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 2,
        parent: 1,
        kind: "select_list",
        range: { start: 0, end: selectEnd },
        semantic: null,
      },
      {
        id: 3,
        parent: 2,
        kind: "select_item",
        range: { start: first, end: first + "field_100".length },
        semantic: null,
      },
      {
        id: 4,
        parent: 3,
        kind: "identifier_expression",
        range: { start: first, end: first + "field_100".length },
        semantic: { kind: "identifier", value: "field_100", quote_style: null },
      },
      {
        id: 5,
        parent: 2,
        kind: "select_item",
        range: { start: second, end: second + secondText.length },
        semantic: null,
      },
      {
        id: 6,
        parent: 5,
        kind: secondLiteral >= 0
          ? "literal_expression"
          : "identifier_expression",
        range: { start: second, end: second + secondText.length },
        semantic: secondLiteral >= 0
          ? { kind: "literal", literal_kind: "string", raw: "'x'" }
          : { kind: "identifier", value: "field_100", quote_style: null },
      },
    ],
  };
};

const makeWhereExpressionDocument = (source: string): SqlDocument => {
  const whereStart = source.indexOf("WHERE");
  const selectValue = source.indexOf("1");
  const comparisonLeft = source.indexOf("1", whereStart);
  const comparisonOperator = source.indexOf("=", comparisonLeft);
  const comparisonRight = source.indexOf("2", comparisonOperator);
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "query",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 2,
        parent: 1,
        kind: "select_list",
        range: { start: source.indexOf("SELECT") + 7, end: selectValue + 1 },
        semantic: null,
      },
      {
        id: 3,
        parent: 2,
        kind: "select_item",
        range: { start: selectValue, end: selectValue + 1 },
        semantic: null,
      },
      {
        id: 4,
        parent: 3,
        kind: "value",
        range: { start: selectValue, end: selectValue + 1 },
        semantic: { kind: "literal", literal_kind: "number", raw: "1" },
      },
      {
        id: 5,
        parent: 1,
        kind: "where_clause",
        range: { start: whereStart, end: source.length },
        semantic: null,
      },
      {
        id: 6,
        parent: 5,
        kind: "binary_expression",
        range: { start: comparisonLeft, end: comparisonRight + 1 },
        semantic: { kind: "binary_operator", operator: "=" },
      },
      {
        id: 7,
        parent: 6,
        kind: "value",
        range: { start: comparisonLeft, end: comparisonLeft + 1 },
        semantic: { kind: "literal", literal_kind: "number", raw: "1" },
      },
      {
        id: 8,
        parent: 6,
        kind: "value",
        range: { start: comparisonRight, end: comparisonRight + 1 },
        semantic: { kind: "literal", literal_kind: "number", raw: "2" },
      },
    ],
  };
};

const makeExpressionLeafDocument = (source: string): SqlDocument => {
  const expressionStart = source.indexOf("field_100 + 1");
  const identifierStart = source.indexOf("field_100", expressionStart);
  const literalStart = source.indexOf("1", identifierStart);
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "query",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 2,
        parent: 1,
        kind: "select",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 3,
        parent: 2,
        kind: "select_list",
        range: {
          start: source.indexOf("SELECT") + 7,
          end: expressionStart + 13,
        },
        semantic: null,
      },
      {
        id: 4,
        parent: 3,
        kind: "select_item",
        range: { start: expressionStart, end: expressionStart + 13 },
        semantic: null,
      },
      {
        id: 5,
        parent: 4,
        kind: "binary_expression",
        range: { start: expressionStart, end: expressionStart + 13 },
        semantic: { kind: "binary_operator", operator: "+" },
      },
      {
        id: 6,
        parent: 5,
        kind: "identifier_expression",
        range: { start: identifierStart, end: identifierStart + 9 },
        semantic: {
          kind: "identifier",
          value: "field_100",
          quote_style: null,
        },
      },
      {
        id: 7,
        parent: 5,
        kind: "value",
        range: { start: literalStart, end: literalStart + 1 },
        semantic: { kind: "literal", literal_kind: "number", raw: "1" },
      },
    ],
  };
};

const makeGroupByDocument = (source: string): SqlDocument => {
  const selectStart = source.indexOf("SELECT") + "SELECT ".length;
  const fromStart = source.indexOf(" FROM ", selectStart);
  const orderStart = source.indexOf("ORDER BY");
  const selectEnd = orderStart >= 0 ? orderStart - 1 : source.length;
  const expressionEnd = fromStart;
  const groupStart = source.indexOf("GROUP BY");
  const groupEnd = groupStart >= 0
    ? source.indexOf(" ORDER BY", groupStart) >= 0
      ? source.indexOf(" ORDER BY", groupStart)
      : source.length
    : -1;
  return {
    contract_version: 14,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "query",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 2,
        parent: 1,
        kind: "select",
        range: { start: 0, end: selectEnd },
        semantic: null,
      },
      {
        id: 3,
        parent: 2,
        kind: "select_list",
        range: { start: selectStart, end: expressionEnd },
        semantic: null,
      },
      {
        id: 4,
        parent: 3,
        kind: "select_item",
        range: { start: selectStart, end: expressionEnd },
        semantic: null,
      },
      {
        id: 5,
        parent: 4,
        kind: "identifier_expression",
        range: { start: selectStart, end: expressionEnd },
        semantic: {
          kind: "identifier",
          value: "field_100",
          quote_style: null,
        },
      },
      ...(groupStart >= 0
        ? [{
          id: 6,
          parent: 2,
          kind: "group_by_clause" as const,
          range: { start: groupStart, end: groupEnd },
          semantic: null,
        }]
        : []),
      ...(orderStart >= 0
        ? [{
          id: 7,
          parent: 1,
          kind: "order_by_clause" as const,
          range: { start: orderStart, end: source.length },
          semantic: null,
        }]
        : []),
    ],
  };
};

const makeOrderByItemsDocument = (source: string): SqlDocument => {
  const orderStart = source.indexOf("ORDER BY");
  const firstStart = source.indexOf("amount", orderStart);
  const comma = source.indexOf(",", firstStart);
  const firstEnd = comma < 0 ? source.length : comma;
  const secondStart = comma < 0 ? -1 : source.indexOf("id", comma);
  const secondEnd = secondStart < 0 ? -1 : source.length;
  const nodes: SqlDocument["nodes"] = [
    {
      id: 0,
      parent: null,
      kind: "query_statement",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 1,
      parent: 0,
      kind: "query",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 2,
      parent: 1,
      kind: "order_by_clause",
      range: { start: orderStart, end: source.length },
      semantic: null,
    },
    {
      id: 3,
      parent: 2,
      kind: "order_by_item",
      range: { start: firstStart, end: firstEnd },
      semantic: { kind: "order_by_item", direction: "asc", nulls_order: null },
    },
    {
      id: 4,
      parent: 3,
      kind: "identifier_expression",
      range: { start: firstStart, end: firstStart + "amount".length },
      semantic: { kind: "identifier", value: "amount", quote_style: null },
    },
  ];
  if (secondStart >= 0) {
    nodes.push(
      {
        id: 5,
        parent: 2,
        kind: "order_by_item",
        range: { start: secondStart, end: secondEnd },
        semantic: {
          kind: "order_by_item",
          direction: "desc",
          nulls_order: null,
        },
      },
      {
        id: 6,
        parent: 5,
        kind: "identifier_expression",
        range: { start: secondStart, end: secondStart + "id".length },
        semantic: { kind: "identifier", value: "id", quote_style: null },
      },
    );
  }
  return {
    contract_version: 17,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes,
  };
};

const makeQueryClauseDocument = (source: string): SqlDocument => {
  const nodes: SqlDocument["nodes"] = [
    {
      id: 0,
      parent: null,
      kind: "query_statement",
      range: { start: 0, end: source.length },
      semantic: null,
    },
    {
      id: 1,
      parent: 0,
      kind: "query",
      range: { start: 0, end: source.length },
      semantic: null,
    },
  ];
  let nextId = 2;
  for (
    const [kind, keyword] of [["limit_clause", "LIMIT"], [
      "offset_clause",
      "OFFSET",
    ]] as const
  ) {
    const start = source.indexOf(keyword);
    if (start < 0) continue;
    const value = Number(
      source.slice(start + keyword.length).trim().split(/\s+/)[0],
    );
    nodes.push({
      id: nextId++,
      parent: 1,
      kind,
      range: { start, end: source.length },
      semantic: { kind: "query_clause_value", value },
    });
  }
  return {
    contract_version: 17,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes,
  };
};

const makeAggregateDocument = (source: string): SqlDocument => {
  const match = /\b(COUNT|SUM|AVG|MIN|MAX)\s*\(/i.exec(source);
  if (!match || match.index === undefined) return makeQueryDocument(source);
  const functionStart = match.index;
  const open = source.indexOf("(", functionStart);
  const close = source.indexOf(")", open);
  const argumentStart = source.indexOf("amount", open);
  const hasArgument = argumentStart >= 0 && argumentStart < close;
  return {
    contract_version: 17,
    source,
    status: "parsed",
    diagnostics: [],
    tokens: [],
    nodes: [
      {
        id: 0,
        parent: null,
        kind: "query_statement",
        range: { start: 0, end: source.length },
        semantic: null,
      },
      {
        id: 1,
        parent: 0,
        kind: "function_expression",
        range: { start: functionStart, end: close + 1 },
        semantic: {
          kind: "function",
          name: [{ value: match[1].toUpperCase(), quote_style: null }],
          argument_count: hasArgument ? 1 : 0,
          distinct: /\(\s*DISTINCT\b/i.test(
            source.slice(functionStart, close + 1),
          ),
          has_filter: false,
          has_over: false,
          has_argument_clauses: false,
          can_replace_argument: hasArgument,
          can_add_argument: !hasArgument,
          can_remove_argument: hasArgument,
        },
      },
      ...(hasArgument
        ? [{
          id: 2,
          parent: 1,
          kind: "identifier_expression" as const,
          range: { start: argumentStart, end: argumentStart + "amount".length },
          semantic: {
            kind: "identifier" as const,
            value: "amount",
            quote_style: null,
          },
        }]
        : []),
    ],
  };
};

describe("SqlDocumentTreeEditor set operation edits", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    parseDocumentMock.mockImplementation(async (source: string) => {
      if (source.startsWith("WITH")) {
        const quoted = source.includes('"rows"');
        return makeCteDocument(
          source,
          quoted ? "rows" : "c",
          quoted ? '"' : null,
        );
      }
      if (source.includes("SUM(CASE")) return makeCaseDocument(source);
      if (source.startsWith("SELECT COUNT(")) return makeCountDocument(source);
      if (
        source.startsWith("SELECT amount,") ||
        source.startsWith("SELECT amount + 1,")
      ) {
        return makeProjectionDocument(source);
      }
      if (
        source.startsWith("SELECT field_100,") ||
        source.startsWith("SELECT (field_100")
      ) {
        return makeBinaryProjectionDocument(source);
      }
      if (source.startsWith("SELECT 1 WHERE")) {
        return makeWhereExpressionDocument(source);
      }
      if (source.startsWith('SELECT 1 FROM "G"')) {
        return source === 'SELECT 1 FROM "G"'
          ? makeScalarProjectionListDocument(source)
          : makeSelectDocument(source);
      }
      if (source === 'SELECT field_100 + 1 FROM "G"') {
        return makeExpressionLeafDocument(source);
      }
      if (source.startsWith('SELECT field_100 FROM "G"')) {
        return makeGroupByDocument(source);
      }
      if (source.includes("CAST(")) {
        return makeCastDocument(
          source,
          source.includes("AS DOUBLE") ? "DOUBLE" : "BIGINT",
          source.includes("TRY_CAST") ? "try_cast" : "cast",
        );
      }
      if (source.includes("WHERE amount IS")) {
        return makePredicateDocument(
          source,
          "is_null",
          source.includes("IS NOT"),
        );
      }
      if (source.includes("WHERE amount IN")) {
        return makePredicateDocument(source, "in", source.includes("NOT IN"));
      }
      if (source.includes("/* keep */ amount")) {
        return makeUnaryDocument(
          source,
          source.includes("SELECT -") ? "-" : "+",
        );
      }
      if (source.includes("BY NAME")) {
        return makeDocument(source, "UNION", "BY NAME");
      }
      if (source.includes("EXCEPT")) {
        return makeDocument(source, "EXCEPT", "DISTINCT");
      }
      return makeDocument(source, "UNION", "ALL");
    });
  });

  it("supports arrow navigation and selection in the SQL syntax tree", async () => {
    const source = "SELECT 1 UNION ALL SELECT 2";
    const [sql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={vi.fn()} />);
    const nodes = await screen.findAllByRole("treeitem");
    expect(nodes).toHaveLength(4);
    expect(nodes[0]).toHaveAttribute("tabindex", "0");

    nodes[0].focus();
    fireEvent.keyDown(nodes[0], { key: "ArrowDown" });
    expect(nodes[1]).toHaveFocus();
    expect(nodes[1]).toHaveAttribute("aria-selected", "true");

    fireEvent.keyDown(nodes[1], { key: "ArrowRight" });
    expect(nodes[2]).toHaveFocus();
    fireEvent.keyDown(nodes[2], { key: "ArrowLeft" });
    expect(nodes[1]).toHaveFocus();
  });

  it("composes a projection from two typed expressions through the Rust protocol", async () => {
    const initialSql = `SELECT field_100, 'x' FROM "G"`;
    const changedSql = `SELECT (field_100\n) || ('x'\n), 'x' FROM "G"`;
    applySemanticEditMock.mockResolvedValue({
      document: makeBinaryProjectionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Selected expression.*field_100/,
      }),
    );
    fireEvent.change(screen.getByLabelText("Left expression"), {
      target: { value: "4" },
    });
    fireEvent.change(screen.getByLabelText("Binary operator"), {
      target: { value: "concat" },
    });
    fireEvent.change(screen.getByLabelText("Right expression"), {
      target: { value: "6" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Build binary expression" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "compose_binary_expression",
        node_id: 3,
        expected_kind: "select_item",
        left_expression_node_id: 4,
        left_expression_expected_kind: "identifier_expression",
        operator: "concat",
        right_expression_node_id: 6,
        right_expression_expected_kind: "literal_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("composes a WHERE expression from same-query typed nodes", async () => {
    const initialSql = "SELECT 1 WHERE 1 = 2";
    const changedSql = "SELECT 1 WHERE (1\n) + (2\n) = 2";
    applySemanticEditMock.mockResolvedValue({
      document: makeWhereExpressionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const treeItems = await screen.findAllByRole("treeitem");
    const target = treeItems.find((item) =>
      item.dataset.sqlDocumentNode === "7"
    );
    expect(target).toBeDefined();
    fireEvent.click(target!);
    fireEvent.change(screen.getByLabelText("Left expression"), {
      target: { value: "4" },
    });
    fireEvent.change(screen.getByLabelText("Binary operator"), {
      target: { value: "add" },
    });
    fireEvent.change(screen.getByLabelText("Right expression"), {
      target: { value: "8" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Build binary expression" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "compose_binary_expression",
        node_id: 7,
        expected_kind: "value",
        left_expression_node_id: 4,
        left_expression_expected_kind: "value",
        operator: "add",
        right_expression_node_id: 8,
        right_expression_expected_kind: "value",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("creates a typed literal leaf from an existing expression node", async () => {
    const initialSql = 'SELECT field_100 + 1 FROM "G"';
    const changedSql = 'SELECT 7 FROM "G"';
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const expression = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "5"
    );
    expect(expression).toBeDefined();
    fireEvent.click(expression!);
    fireEvent.change(screen.getByLabelText("Expression value"), {
      target: { value: "literal" },
    });
    fireEvent.change(screen.getByLabelText("Value type"), {
      target: { value: "number" },
    });
    fireEvent.input(screen.getByLabelText("Number"), {
      target: { value: "7" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_literal",
        node_id: 5,
        expected_kind: "binary_expression",
        literal: { kind: "number", value: "7" },
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("wraps a selected typed expression in CAST through Rust", async () => {
    const initialSql = 'SELECT field_100 + 1 FROM "G"';
    const changedSql = 'SELECT CAST(field_100 + 1 AS DOUBLE) FROM "G"';
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const expression = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "5"
    );
    expect(expression).toBeDefined();
    fireEvent.click(expression!);
    fireEvent.change(screen.getByLabelText("Wrap expression in CAST"), {
      target: { value: "double" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "wrap_expression_in_cast",
        node_id: 5,
        expected_kind: "binary_expression",
        target_type: "double",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("builds a registered DISTINCT aggregate around a projection expression", async () => {
    const initialSql = 'SELECT field_100 + 1 FROM "G"';
    const changedSql = 'SELECT SUM(DISTINCT field_100 + 1) FROM "G"';
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const expression = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "5"
    );
    expect(expression).toBeDefined();
    fireEvent.click(expression!);
    fireEvent.change(screen.getByLabelText("Aggregate function"), {
      target: { value: "sum" },
    });
    fireEvent.click(screen.getByLabelText("DISTINCT"));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "wrap_expression_in_aggregate",
        node_id: 5,
        expected_kind: "binary_expression",
        function: "sum",
        distinct: true,
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("builds a typed BETWEEN predicate from an expression node", async () => {
    const initialSql = 'SELECT field_100 + 1 FROM "G"';
    const changedSql = 'SELECT (field_100 + 1) BETWEEN NULL AND NULL FROM "G"';
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const expression = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "5"
    );
    expect(expression).toBeDefined();
    fireEvent.click(expression!);
    fireEvent.change(screen.getByLabelText("Predicate"), {
      target: { value: "between" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "wrap_expression_in_predicate",
        node_id: 5,
        expected_kind: "binary_expression",
        template: "between",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("wraps a typed expression in an editable searched CASE", async () => {
    const initialSql = 'SELECT field_100 + 1 FROM "G"';
    const changedSql =
      'SELECT CASE WHEN TRUE THEN field_100 + 1 ELSE NULL END FROM "G"';
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const expression = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "5"
    );
    expect(expression).toBeDefined();
    fireEvent.click(expression!);
    fireEvent.click(screen.getByRole("button", { name: "Wrap in CASE" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "wrap_expression_in_case",
        node_id: 5,
        expected_kind: "binary_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds WHERE from a SELECT node before later clauses", async () => {
    const initialSql = 'SELECT 1 FROM "G" GROUP BY 1 ORDER BY 1 LIMIT 2';
    const changedSql =
      'SELECT 1 FROM "G" WHERE TRUE GROUP BY 1 ORDER BY 1 LIMIT 2';
    applySemanticEditMock.mockResolvedValue({
      document: makeSelectDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);
    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const selectNode = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "2"
    );
    expect(selectNode).toBeDefined();
    fireEvent.click(selectNode!);
    fireEvent.click(
      screen.getByRole("button", { name: "Add filter clause" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_where_clause",
        node_id: 2,
        expected_kind: "select",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a typed expression to GROUP BY before later clauses", async () => {
    const initialSql = 'SELECT field_100 FROM "G" ORDER BY field_100';
    const changedSql =
      'SELECT field_100 FROM "G" GROUP BY field_100 ORDER BY field_100';
    applySemanticEditMock.mockResolvedValue({
      document: makeGroupByDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const selectNode = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "2"
    );
    expect(selectNode).toBeDefined();
    fireEvent.click(selectNode!);
    fireEvent.change(screen.getByLabelText("Group by expression"), {
      target: { value: "5" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Add group expression" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_group_by_expression",
        node_id: 2,
        expected_kind: "select",
        expression_node_id: 5,
        expression_expected_kind: "identifier_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a typed expression to ORDER BY from the query node", async () => {
    const initialSql = 'SELECT field_100 FROM "G" LIMIT 10';
    const changedSql =
      'SELECT field_100 FROM "G" ORDER BY field_100 DESC NULLS LAST LIMIT 10';
    applySemanticEditMock.mockResolvedValue({
      document: makeGroupByDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const queryNode = (await screen.findAllByRole("treeitem")).find((item) =>
      item.dataset.sqlDocumentNode === "1"
    );
    expect(queryNode).toBeDefined();
    fireEvent.click(queryNode!);
    fireEvent.change(screen.getByLabelText("Direction"), {
      target: { value: "desc" },
    });
    fireEvent.change(screen.getByLabelText("NULL placement"), {
      target: { value: "last" },
    });
    fireEvent.change(screen.getByLabelText("Order by expression"), {
      target: { value: "5" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Add order expression" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_order_by_expression",
        node_id: 1,
        expected_kind: "query",
        expression_node_id: 5,
        expression_expected_kind: "identifier_expression",
        direction: "desc",
        nulls_order: "last",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("replaces an expression from a typed node in the same query", async () => {
    const initialSql = `SELECT field_100, 'x' FROM "G"`;
    const changedSql = `SELECT field_100, field_100 FROM "G"`;
    applySemanticEditMock.mockResolvedValue({
      document: makeBinaryProjectionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Value.*'x'/ }),
    );
    fireEvent.change(screen.getByLabelText("Choose expression"), {
      target: { value: "4" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Use selected expression" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_expression_from_node",
        node_id: 6,
        expected_kind: "literal_expression",
        expression_node_id: 4,
        expression_expected_kind: "identifier_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("sends a typed set operator and quantifier edit through the Rust protocol", async () => {
    const initialSql = "SELECT 1 UNION ALL SELECT 2";
    const changedSql = "SELECT 1 EXCEPT DISTINCT SELECT 2";
    applySemanticEditMock.mockResolvedValue({
      document: makeDocument(changedSql, "EXCEPT", "DISTINCT"),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const setOperation = await screen.findByRole("treeitem", {
      name: /Set operation/,
    });
    fireEvent.click(setOperation);
    fireEvent.change(screen.getByLabelText("Set operator"), {
      target: { value: "except" },
    });
    fireEvent.change(screen.getByLabelText("Set quantifier"), {
      target: { value: "distinct" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_set_operation",
        node_id: 1,
        expected_kind: "set_operation",
        operator: "except",
        quantifier: "distinct",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("appends a set-operation seed from a query node through Rust", async () => {
    const initialSql = "SELECT 1";
    const changedSql = "(SELECT 1\n) UNION ALL SELECT 1";
    parseDocumentMock.mockImplementation(async (source: string) =>
      source === initialSql
        ? makeQueryDocument(source)
        : makeDocument(source, "UNION", "ALL")
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeDocument(changedSql, "UNION", "ALL"),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Query: SELECT 1/ }),
    );
    fireEvent.change(screen.getByLabelText("Set operator"), {
      target: { value: "union" },
    });
    fireEvent.change(screen.getByLabelText("Set quantifier"), {
      target: { value: "all" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Add set operation" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_set_operation_operand",
        node_id: 1,
        expected_kind: "query",
        operator: "union",
        quantifier: "all",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("removes an explicitly selected set-operation operand through Rust", async () => {
    const initialSql = "SELECT 1 UNION ALL SELECT 2";
    const changedSql = "SELECT 1";
    parseDocumentMock.mockImplementation(async (source: string) =>
      source === initialSql
        ? makeDocument(source, "UNION", "ALL")
        : makeQueryDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Set operation/ }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Remove right operand" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "remove_set_operation_operand",
        node_id: 1,
        expected_kind: "set_operation",
        keep: "left",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("swaps set-operation operands through a guarded Rust edit", async () => {
    const initialSql = "SELECT 1 UNION ALL SELECT 2";
    const changedSql = "SELECT 2 UNION ALL SELECT 1";
    applySemanticEditMock.mockResolvedValue({
      document: makeDocument(changedSql, "UNION", "ALL"),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Set operation/ }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Swap operands" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "swap_set_operation_operands",
        node_id: 1,
        expected_kind: "set_operation",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("supports typed BY NAME set quantifiers through Rust", async () => {
    const source = "SELECT 1 UNION BY NAME SELECT 2";
    const changedSql = "SELECT 1 UNION ALL BY NAME SELECT 2";
    applySemanticEditMock.mockResolvedValue({
      document: makeDocument(changedSql, "UNION", "ALL BY NAME"),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Set operation/,
      }),
    );
    fireEvent.change(screen.getByLabelText("Set quantifier"), {
      target: { value: "all_by_name" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "replace_set_operation",
        node_id: 1,
        expected_kind: "set_operation",
        operator: "union",
        quantifier: "all_by_name",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("replaces a set-operation operand with another parsed query", async () => {
    const source = "SELECT 1 UNION ALL SELECT 2";
    const changedSql = "(SELECT 2) UNION ALL SELECT 2";
    parseDocumentMock.mockImplementation(async (sql: string) =>
      makeSetOperationTreeDocument(sql)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeSetOperationTreeDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Left query: SELECT 1/ }),
    );
    fireEvent.change(screen.getByLabelText("Set operand query"), {
      target: { value: "6" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "replace_set_operation_operand",
        node_id: 2,
        expected_kind: "set_operation_left",
        expression_node_id: 6,
        expression_expected_kind: "query",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("replaces a CTE body with another query from the parsed document", async () => {
    const source = "WITH rows AS (SELECT 1), other AS (SELECT 2) SELECT 3";
    const changedSql = "WITH rows AS (SELECT 2), other AS (SELECT 2) SELECT 3";
    parseDocumentMock.mockImplementation(async (sql: string) =>
      makeTwoCteDocument(sql)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeTwoCteDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Common table expression: rows/,
      }),
    );
    fireEvent.change(screen.getByLabelText("CTE query"), {
      target: { value: "6" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Replace CTE query" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "replace_common_table_expression_query",
        node_id: 3,
        expected_kind: "common_table_expression",
        query_node_id: 6,
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("moves a CTE to an adjacent position through Rust", async () => {
    const source = "WITH rows AS (SELECT 1), other AS (SELECT 2) SELECT 3";
    const changedSql = "WITH other AS (SELECT 2), rows AS (SELECT 1) SELECT 3";
    parseDocumentMock.mockImplementation(async (sql: string) =>
      makeTwoCteDocument(sql)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeTwoCteDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const cte = (await screen.findAllByRole("treeitem", {
      name: /Common table expression:/,
    })).find((item) => item.dataset.sqlDocumentNode === "5");
    expect(cte).toBeDefined();
    fireEvent.click(cte!);
    fireEvent.click(
      screen.getByRole("button", { name: "Move CTE earlier" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "move_common_table_expression",
        node_id: 5,
        expected_kind: "common_table_expression",
        to_index: 0,
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a Form to a SELECT without FROM through the typed operation", async () => {
    const source = "SELECT 1";
    const changedSql = 'SELECT 1 FROM "G"';
    parseDocumentMock.mockImplementation(async (sql: string) =>
      makeSelectDocument(sql)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeSelectDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(source);

    render(() => (
      <SqlDocumentTreeEditor
        sql={sql}
        onSqlChange={setSql}
        relationOptions={() => [{ relation: "G", label: "G" }]}
      />
    ));
    const select = (await screen.findAllByRole("treeitem", {
      name: /SELECT/,
    })).find((item) => item.dataset.sqlDocumentNode === "2");
    expect(select).toBeDefined();
    fireEvent.click(select!);
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "G" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Add FROM relation" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "add_from_relation",
        node_id: 2,
        expected_kind: "select",
        factor: {
          kind: "table",
          name: [{ value: "G", quote_style: '"' }],
          alias: null,
        },
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("wraps an expression with a typed IN subquery", async () => {
    const source = 'SELECT field_100 + 1 FROM "G"';
    const changedSql = 'SELECT field_100 IN (SELECT 1) + 1 FROM "G"';
    parseDocumentMock.mockImplementation(async (sql: string) =>
      makeExpressionLeafDocument(sql)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeExpressionLeafDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Identifier: field_100/,
      }),
    );
    fireEvent.change(screen.getByLabelText("Subquery condition"), {
      target: { value: "in" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "wrap_expression_in_subquery_predicate",
        node_id: 6,
        expected_kind: "identifier_expression",
        template: "in",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("replaces an expression with a typed EXISTS subquery", async () => {
    const source = 'SELECT field_100 + 1 FROM "G"';
    const changedSql = 'SELECT EXISTS (SELECT 1) + 1 FROM "G"';
    parseDocumentMock.mockImplementation(async (sql: string) =>
      makeExpressionLeafDocument(sql)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeExpressionLeafDocument(changedSql),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Identifier: field_100/,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Use EXISTS" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "replace_expression_with_exists_subquery",
        node_id: 6,
        expected_kind: "identifier_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("sends a typed CAST target edit through the Rust protocol", async () => {
    const initialSql = "SELECT CAST(amount AS BIGINT)";
    const changedSql = "SELECT CAST(amount AS DOUBLE)";
    applySemanticEditMock.mockResolvedValue({
      document: makeCastDocument(changedSql, "DOUBLE"),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /CAST: CAST\(amount AS BIGINT\)/,
      }),
    );
    fireEvent.change(screen.getByLabelText("Cast target type"), {
      target: { value: "double" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_cast_target_type",
        node_id: 1,
        expected_kind: "cast_expression",
        target_type: "double",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("edits a literal inside a CASE aggregate expression through Rust", async () => {
    const initialSql =
      "SELECT SUM(CASE WHEN field_100 = 'not-a-number' THEN 10 ELSE 1 END) AS total FROM tasks";
    const changedSql =
      "SELECT SUM(CASE WHEN field_100 = 'not-a-number' THEN 20 ELSE 1 END) AS total FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeCaseDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Value: 10/,
      }),
    );
    fireEvent.input(screen.getByLabelText("Number"), {
      target: { value: "20" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_literal",
        node_id: 6,
        expected_kind: "value",
        literal: { kind: "number", value: "20" },
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a CASE branch from the syntax tree controls", async () => {
    const initialSql =
      "SELECT SUM(CASE WHEN field_100 = 'not-a-number' THEN 10 ELSE 1 END) AS total FROM tasks";
    const changedSql =
      "SELECT SUM(CASE WHEN field_100 = 'not-a-number' THEN 10 WHEN TRUE THEN NULL ELSE 1 END) AS total FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeCaseDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /CASE: CASE WHEN field_100 = 'not-a-number'/,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Add CASE branch" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_case_branch",
        node_id: 2,
        expected_kind: "case_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a COUNT argument from another typed expression node", async () => {
    const initialSql = "SELECT COUNT(amount), amount FROM tasks";
    const changedSql = "SELECT COUNT(amount, amount), amount FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeCountDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Function: COUNT\(amount\)/,
      }),
    );
    fireEvent.change(screen.getByLabelText("Argument"), {
      target: { value: "1" },
    });
    fireEvent.change(screen.getByLabelText("Expression"), {
      target: { value: "3" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "set_function_argument",
        node_id: 1,
        expected_kind: "function_expression",
        argument_index: 1,
        expression_node_id: 3,
        expression_expected_kind: "identifier_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("removes the last COUNT argument through the syntax-tree controls", async () => {
    const initialSql = "SELECT COUNT(amount) FROM tasks";
    const changedSql = "SELECT COUNT() FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeCountDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Function: COUNT\(amount\)/,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Remove final argument" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "remove_last_function_argument",
        node_id: 1,
        expected_kind: "function_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("replaces a selected projection with another typed expression", async () => {
    const initialSql = "SELECT amount, amount + 1 FROM tasks";
    const changedSql = "SELECT amount + 1, amount + 1 FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeProjectionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Selected expression: amount$/,
      }),
    );
    fireEvent.change(screen.getByLabelText("Expression"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "set_projection_expression",
        node_id: 2,
        expected_kind: "select_item",
        expression_node_id: 5,
        expression_expected_kind: "binary_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds another SELECT item from a typed expression in the query", async () => {
    const initialSql = "SELECT amount, amount + 1 FROM tasks";
    const changedSql = "SELECT amount, amount + 1, amount + 1 FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeProjectionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Selected columns/ }),
    );
    fireEvent.change(screen.getByLabelText("Expression"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_projection_expression",
        node_id: 1,
        expected_kind: "select_list",
        expression_node_id: 5,
        expression_expected_kind: "binary_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a scalar subquery projection through the Rust protocol", async () => {
    const initialSql = 'SELECT 1 FROM "G"';
    const changedSql = 'SELECT 1, (SELECT 1) FROM "G"';
    applySemanticEditMock.mockResolvedValue({
      document: makeSelectDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Selected columns/ }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Add scalar subquery" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_scalar_subquery_projection",
        node_id: 3,
        expected_kind: "select_list",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds another SELECT item from a typed expression in the query", async () => {
    const initialSql = "SELECT amount, amount + 1 FROM tasks";
    const changedSql = "SELECT amount, amount + 1, amount + 1 FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeProjectionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Selected columns/ }),
    );
    fireEvent.change(screen.getByLabelText("Expression"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_projection_expression",
        node_id: 1,
        expected_kind: "select_list",
        expression_node_id: 5,
        expression_expected_kind: "binary_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("moves a typed projection item with one guarded semantic operation", async () => {
    const initialSql = "SELECT amount, amount + 1 FROM tasks";
    const changedSql = "SELECT amount + 1, amount FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeProjectionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Selected expression: amount$/,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Move projection down" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "move_projection_item",
        node_id: 2,
        expected_kind: "select_item",
        to_index: 1,
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("removes a typed projection item without opening a second editor", async () => {
    const initialSql = "SELECT amount, amount + 1 FROM tasks";
    const changedSql = "SELECT amount FROM tasks";
    applySemanticEditMock.mockResolvedValue({
      document: makeProjectionDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Selected expression: amount \+ 1$/,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Remove projection" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "remove_projection_item",
        node_id: 4,
        expected_kind: "select_item",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("edits TRY_CAST target types without replacing the cast form", async () => {
    const source = "SELECT TRY_CAST(amount AS BIGINT)";
    const changedSql = "SELECT TRY_CAST(amount AS DOUBLE)";
    applySemanticEditMock.mockResolvedValue({
      document: makeCastDocument(changedSql, "DOUBLE", "try_cast"),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(source);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /CAST: TRY_CAST\(amount AS BIGINT\)/,
      }),
    );
    fireEvent.change(screen.getByLabelText("Cast target type"), {
      target: { value: "double" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(source, {
      contract_version: 14,
      expected_source: source,
      edits: [{
        kind: "replace_cast_target_type",
        node_id: 1,
        expected_kind: "cast_expression",
        target_type: "double",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("renames a CTE and applies the exact typed name through Rust", async () => {
    const initialSql = "WITH c AS (SELECT 1 AS n) SELECT n FROM c";
    const changedSql = 'WITH "rows" AS (SELECT 1 AS n) SELECT n FROM "rows"';
    applySemanticEditMock.mockResolvedValue({
      document: makeCteDocument(changedSql, "rows", '"'),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Common table expression/ }),
    );
    fireEvent.input(screen.getByLabelText("CTE name"), {
      target: { value: "rows" },
    });
    fireEvent.change(screen.getByLabelText("Quote style 1"), {
      target: { value: '"' },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "rename_common_table_expression",
        node_id: 2,
        expected_kind: "common_table_expression",
        name: { value: "rows", quote_style: '"' },
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a CTE from the selected query node through the Rust protocol", async () => {
    const initialSql = "SELECT 1";
    const changedSql = "WITH rows AS (SELECT 1) SELECT 1";
    parseDocumentMock.mockImplementation(async (source: string) =>
      source === initialSql
        ? makeQueryDocument(source)
        : makeCteDocument(source, "rows", null)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeCteDocument(changedSql, "rows", null),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Query: SELECT 1/ }),
    );
    fireEvent.input(screen.getByLabelText("CTE name"), {
      target: { value: "rows" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add CTE" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_common_table_expression",
        node_id: 1,
        expected_kind: "query",
        name: { value: "rows", quote_style: null },
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("removes a selected CTE through the Rust protocol", async () => {
    const initialSql = "WITH c AS (SELECT 1) SELECT 2";
    const changedSql = "SELECT 2";
    parseDocumentMock.mockImplementation(async (source: string) =>
      source.startsWith("WITH")
        ? makeCteDocument(source, "c", null)
        : makeQueryDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Common table expression/ }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove CTE" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "remove_common_table_expression",
        node_id: 2,
        expected_kind: "common_table_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("toggles predicate negation through the Rust semantic edit protocol", async () => {
    const initialSql = 'SELECT amount FROM "G" WHERE amount IS NULL';
    const changedSql = 'SELECT amount FROM "G" WHERE amount IS NOT NULL';
    applySemanticEditMock.mockResolvedValue({
      document: makePredicateDocument(changedSql, "is_null", true),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", {
        name: /Predicate: amount IS NULL/,
      }),
    );
    fireEvent.click(screen.getByLabelText("Negated"));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_predicate_negation",
        node_id: 1,
        expected_kind: "predicate_expression",
        negated: true,
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds an IN list value from its predicate node", async () => {
    const initialSql = 'SELECT amount FROM "G" WHERE amount IN (1, 2)';
    const changedSql = 'SELECT amount FROM "G" WHERE amount IN (1, 2, NULL)';
    applySemanticEditMock.mockResolvedValue({
      document: makePredicateDocument(changedSql, "in", false),
      undo: { contract_version: 14, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Predicate: amount IN/ }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Add IN value" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "append_in_list_value",
        node_id: 1,
        expected_kind: "predicate_expression",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("edits arithmetic unary signs through the Rust semantic edit protocol", async () => {
    const initialSql = "SELECT - /* keep */ amount";
    const changedSql = "SELECT + /* keep */ amount";
    applySemanticEditMock.mockResolvedValue({
      document: makeUnaryDocument(changedSql, "+"),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Unary expression: -/ }),
    );
    fireEvent.change(screen.getByLabelText("Unary sign"), {
      target: { value: "positive" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_unary_sign",
        node_id: 1,
        expected_kind: "unary_expression",
        sign: "positive",
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("adds a second Form relation with a typed LEFT JOIN ON edit", async () => {
    const initialSql = 'SELECT g.id FROM "G" AS g WHERE g.id = 1';
    const changedSql =
      'SELECT g.id FROM "G" AS g LEFT JOIN "H" AS h ON g.id = 1 WHERE g.id = 1';
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeRelationDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeRelationDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => (
      <SqlDocumentTreeEditor
        sql={sql}
        onSqlChange={setSql}
        relationOptions={() => [
          { relation: "G", label: "G" },
          { relation: "H", label: "H" },
        ]}
      />
    ));
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Relations:/ }),
    );
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "H" },
    });
    fireEvent.input(screen.getByLabelText("Alias"), {
      target: { value: "h" },
    });
    fireEvent.change(screen.getByLabelText("Join"), {
      target: { value: "left" },
    });
    fireEvent.change(screen.getByLabelText("Condition"), {
      target: { value: "on" },
    });
    const onExpression = screen.getByLabelText("ON expression");
    const conditionOption = Array.from(
      (onExpression as HTMLSelectElement).options,
    ).find((option) => option.textContent === "g.id = 1");
    expect(conditionOption).toBeDefined();
    fireEvent.change(onExpression, {
      target: { value: conditionOption!.value },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add join" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "add_join",
        node_id: 7,
        expected_kind: "relation_tree",
        join: {
          operator: "left",
          relation: {
            kind: "table",
            name: [{ value: "H", quote_style: '"' }],
            alias: { value: "h", quote_style: null },
          },
          constraint: {
            kind: "on",
            expression_node_id: 11,
            expression_expected_kind: "binary_expression",
          },
        },
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("replaces an existing table factor with a selected Form", async () => {
    const initialSql = 'SELECT g.id FROM "G" AS g';
    const changedSql = 'SELECT g.id FROM "H" AS g';
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeRelationDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeRelationDocument(changedSql),
      undo: {
        contract_version: 14,
        expected_source: changedSql,
        edits: [],
      },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => (
      <SqlDocumentTreeEditor
        sql={sql}
        onSqlChange={setSql}
        relationOptions={() => [
          { relation: "G", label: "G" },
          { relation: "H", label: "H" },
        ]}
      />
    ));
    fireEvent.click(
      await screen.findByRole("treeitem", { name: /Table source:/ }),
    );
    fireEvent.change(screen.getByLabelText("Form"), {
      target: { value: "H" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Replace relation" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => expect(applySemanticEditMock).toHaveBeenCalledOnce());
    expect(applySemanticEditMock).toHaveBeenCalledWith(initialSql, {
      contract_version: 14,
      expected_source: initialSql,
      edits: [{
        kind: "replace_table_factor",
        node_id: 8,
        expected_kind: "table_factor",
        factor: {
          kind: "table",
          name: [{ value: "H", quote_style: '"' }],
          alias: { value: "g", quote_style: null },
        },
      }],
    });
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("edits ORDER BY direction and null placement through typed node operations", async () => {
    const initialSql = 'SELECT amount FROM "G" ORDER BY amount ASC';
    const changedSql = 'SELECT amount FROM "G" ORDER BY amount DESC NULLS LAST';
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeOrderByItemsDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeOrderByItemsDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const item = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "3"
    );
    fireEvent.click(item!);
    fireEvent.change(screen.getByRole("combobox", { name: "Direction" }), {
      target: { value: "desc" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "NULL placement" }), {
      target: { value: "last" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "set_order_by_options",
            node_id: 3,
            expected_kind: "order_by_item",
            direction: "desc",
            nulls_order: "last",
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("replaces an ORDER BY expression from another typed item", async () => {
    const initialSql = 'SELECT amount FROM "G" ORDER BY amount ASC, id DESC';
    const changedSql =
      'SELECT amount FROM "G" ORDER BY amount ASC, amount DESC';
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeOrderByItemsDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeOrderByItemsDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const secondItem = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "5"
    );
    fireEvent.click(secondItem!);
    fireEvent.change(
      screen.getByRole("combobox", { name: "Order by expression" }),
      { target: { value: "4" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "set_order_by_expression",
            node_id: 5,
            expected_kind: "order_by_item",
            expression_node_id: 4,
            expression_expected_kind: "identifier_expression",
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("moves and removes ORDER BY items through guarded operations", async () => {
    const initialSql = 'SELECT amount FROM "G" ORDER BY amount ASC, id DESC';
    const movedSql = 'SELECT amount FROM "G" ORDER BY id DESC, amount ASC';
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeOrderByItemsDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeOrderByItemsDocument(movedSql),
      undo: { contract_version: 17, expected_source: movedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const secondItem = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "5"
    );
    fireEvent.click(secondItem!);
    fireEvent.click(
      screen.getByRole("button", { name: "Move order key earlier" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "move_order_by_item",
            node_id: 5,
            expected_kind: "order_by_item",
            to_index: 0,
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(movedSql));
  });

  it("removes an ORDER BY item through a guarded operation", async () => {
    const initialSql = 'SELECT amount FROM "G" ORDER BY amount ASC, id DESC';
    const changedSql = 'SELECT amount FROM "G" ORDER BY id DESC';
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeOrderByItemsDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeOrderByItemsDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const firstItem = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "3"
    );
    fireEvent.click(firstItem!);
    fireEvent.click(screen.getByRole("button", { name: "Remove order key" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "remove_order_by_item",
            node_id: 3,
            expected_kind: "order_by_item",
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("sets a row limit on a query without removing other clauses", async () => {
    const initialSql = "SELECT 1";
    const changedSql = "SELECT 1 LIMIT 20";
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeQueryClauseDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryClauseDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const query = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "1"
    );
    fireEvent.click(query!);
    fireEvent.input(screen.getByLabelText("Limit"), {
      target: { value: "20" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Set row limit" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "set_limit_value",
            node_id: 1,
            expected_kind: "query",
            value: 20,
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("sets an offset on a query through a typed operation", async () => {
    const initialSql = "SELECT 1";
    const changedSql = "SELECT 1 OFFSET 5";
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeQueryClauseDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryClauseDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const query = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "1"
    );
    fireEvent.click(query!);
    fireEvent.input(screen.getByLabelText("Offset"), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Set row offset" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "set_offset_value",
            node_id: 1,
            expected_kind: "query",
            value: 5,
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("removes an existing LIMIT through a typed query operation", async () => {
    const initialSql = "SELECT 1 LIMIT 10 OFFSET 4";
    const changedSql = "SELECT 1 OFFSET 4";
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeQueryClauseDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryClauseDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const query = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "1"
    );
    fireEvent.click(query!);
    fireEvent.click(screen.getByRole("button", { name: "Remove row limit" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "remove_limit",
            node_id: 1,
            expected_kind: "query",
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("removes an existing OFFSET through a typed query operation", async () => {
    const initialSql = "SELECT 1 LIMIT 10 OFFSET 4";
    const changedSql = "SELECT 1 LIMIT 10";
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeQueryClauseDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryClauseDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const query = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "1"
    );
    fireEvent.click(query!);
    fireEvent.click(screen.getByRole("button", { name: "Remove row offset" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "remove_offset",
            node_id: 1,
            expected_kind: "query",
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("updates aggregate functions, DISTINCT, and removal with typed operations", async () => {
    const initialSql = "SELECT COUNT(amount)";
    const sumSql = "SELECT SUM(amount)";
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeAggregateDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeAggregateDocument(sumSql),
      undo: { contract_version: 17, expected_source: sumSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const aggregate = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "1"
    );
    fireEvent.click(aggregate!);
    fireEvent.change(
      screen.getByRole("combobox", { name: "Aggregate function" }),
      {
        target: { value: "sum" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "set_aggregate_function",
            node_id: 1,
            expected_kind: "function_expression",
            function: "sum",
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(sumSql));
  });

  it("toggles aggregate DISTINCT through the typed operation", async () => {
    const initialSql = "SELECT COUNT(amount)";
    const changedSql = "SELECT COUNT(DISTINCT amount)";
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeAggregateDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeAggregateDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const aggregate = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "1"
    );
    fireEvent.click(aggregate!);
    fireEvent.click(screen.getByLabelText("DISTINCT"));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "set_aggregate_distinct",
            node_id: 1,
            expected_kind: "function_expression",
            distinct: true,
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });

  it("removes an aggregate wrapper with the typed operation", async () => {
    const initialSql = "SELECT COUNT(amount)";
    const changedSql = "SELECT amount";
    parseDocumentMock.mockImplementation(async (source: string) =>
      makeAggregateDocument(source)
    );
    applySemanticEditMock.mockResolvedValue({
      document: makeQueryDocument(changedSql),
      undo: { contract_version: 17, expected_source: changedSql, edits: [] },
    });
    const [sql, setSql] = createSignal(initialSql);

    render(() => <SqlDocumentTreeEditor sql={sql} onSqlChange={setSql} />);
    const aggregate = (await screen.findAllByRole("treeitem")).find((node) =>
      node.dataset.sqlDocumentNode === "1"
    );
    fireEvent.click(aggregate!);
    fireEvent.click(screen.getByRole("button", { name: "Remove aggregate" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() =>
      expect(applySemanticEditMock).toHaveBeenCalledWith(
        initialSql,
        {
          contract_version: 17,
          expected_source: initialSql,
          edits: [{
            kind: "remove_aggregate",
            node_id: 1,
            expected_kind: "function_expression",
          }],
        },
      )
    );
    await waitFor(() => expect(sql()).toBe(changedSql));
  });
});
