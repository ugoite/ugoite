import { http, HttpResponse } from "msw";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applySqlDocumentEditTransaction,
  applySqlDocumentNodeEditTransaction,
  applySqlDocumentSemanticEditTransaction,
  buildSpreadsheetCsvRequest,
  canonicalizeCompositionDocument,
  collectSqlFormNameReferences,
  compileVariableGraph,
  compositionFormQuerySql,
  encodeSpreadsheetCsv,
  getWasmSupportedOperations,
  parseSqlDocument,
  parseVariableGraphInput,
  parseVariableGraphInputSync,
  prepareApiRequest,
  prepareVariableGraphInputParser,
  protocolFetch,
  protocolFetchResponse,
  renameSqlDocumentParameter,
  spreadsheetCsvRequestBytes,
  type SqlDocumentEditErrorCode,
  UGOITE_API_OPERATIONS,
  UGOITE_WASM_PROTOCOL_VERSION,
  UgoiteApiError,
  validateAssetReference,
  type VariableGraph,
} from "./protocol";
import { testApiUrl } from "~/test/http-origin";
import { server } from "~/test/mocks/server";
import type { CompositionSaveResponse } from "../composition-api";
import { compositionApi } from "../composition-api";
import { exactIntegerJsonValue } from "../exact-integer";

const validReference = {
  asset_id: "01900000-0000-7000-8000-000000000001",
  name: "report.pdf",
  media_type: "application/pdf",
  size_bytes: 10,
  sha256: "a".repeat(64),
};

const typedCompositionDocument = {
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: "Monthly expenses",
  spec: {
    variables: [
      {
        id: "month_start",
        kind: "input",
        value_type: { scalar_type: "date", nullable: false },
        required: true,
        default: { kind: "date", value: "2026-10-01" },
      },
      {
        id: "expense_rows",
        kind: "query",
        returns: {
          kind: "table",
          columns: [{
            name: "amount",
            value_type: { scalar_type: "decimal", nullable: true },
          }],
        },
        query: {
          kind: "inline_sql",
          sql: 'SELECT amount FROM "Expense" WHERE "Date" >= $month_start',
          arguments: [{ name: "month_start", variable: "month_start" }],
          form_bindings: [{
            relation: "Expense",
            form_id: "00000000-0000-7000-8000-000000000010",
          }],
          schema_fingerprint:
            "99a980a869d3c01c212a4d175e0616b3237ddfc19b35a6f027829f7cadad3d67",
        },
      },
    ],
    components: [
      { id: "month_start_input", kind: "input", variable: "month_start" },
      { id: "results_table", kind: "table", variable: "expense_rows" },
    ],
    layout: {
      kind: "flow",
      rows: [{
        id: "main",
        items: [
          { kind: "component", component: "month_start_input" },
          { kind: "component", component: "results_table" },
        ],
      }],
    },
  },
};

const prereleaseSourceShapedCompositionDocument = {
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: "Monthly expenses",
  spec: {
    variables: [{
      id: "month_start",
      type: "date",
      required: true,
      default: "2026-10-01",
    }],
    sources: [{
      id: "sql_results",
      kind: "saved_sql",
      entry_id: "00000000-0000-7000-8000-000000000020",
      revision_id: "00000000-0000-7000-8000-000000000021",
      expected_result: [{ name: "amount", type: "json" }],
      schema_fingerprint:
        "ed3bd3fbcd9aa04517ff6d9dbb8a7a6de4eede34a704908debdea72b5c0b1ec5",
      variables: { month_start: { variable: "month_start" } },
    }],
    components: [
      { id: "month_start_input", kind: "input", variable: "month_start" },
      { id: "results_table", kind: "table", source: "sql_results" },
    ],
    layout: {
      kind: "flow",
      rows: [{
        id: "main",
        items: [
          { kind: "component", component: "month_start_input" },
          { kind: "component", component: "results_table" },
        ],
      }],
    },
  },
};

describe("portable Ugoite API protocol WASM", () => {
  afterEach(() => vi.restoreAllMocks());

  it("requires preparation before synchronously parsing a graph Input", async () => {
    vi.resetModules();
    const freshProtocol = await import("./protocol");

    expect(() =>
      freshProtocol.parseVariableGraphInputSync(
        { scalar_type: "integer", nullable: false },
        "1",
      )
    )
      .toThrow("Variable graph input parser is not prepared");
  });

  it("constructs Form table and explicit metric SQL through Rust with quoted identities", async () => {
    expect(await compositionFormQuerySql('form"identity')).toEqual({
      sql: 'SELECT * FROM "form""identity"',
    });
    expect(await compositionFormQuerySql("form_id", { kind: "count" })).toEqual(
      { sql: 'SELECT COUNT(*) AS value FROM "form_id"' },
    );
    expect(
      await compositionFormQuerySql("form_id", { kind: "sum", column: 'a"b' }),
    ).toEqual({ sql: 'SELECT SUM("a""b") AS value FROM "form_id"' });
    await expect(compositionFormQuerySql("")).rejects.toThrow();
  });

  it("canonicalizes a strict variable-graph Composition through Rust/WASM", async () => {
    const canonical = await canonicalizeCompositionDocument(
      typedCompositionDocument,
    );

    expect(canonical.document).toMatchObject({
      format: "ugoite.composition",
      format_version: 1,
      name: "Monthly expenses",
      spec: {
        variables: [
          {
            id: "month_start",
            kind: "input",
            value_type: { scalar_type: "date", nullable: false },
          },
          { id: "expense_rows", kind: "query", query: { kind: "inline_sql" } },
        ],
        components: [
          {
            id: "month_start_input",
            kind: "input",
            variable: "month_start",
          },
          {
            id: "results_table",
            kind: "table",
            variable: "expense_rows",
          },
        ],
      },
    });
    expect(canonical.canonical_yaml).toContain(
      'SELECT amount FROM "Expense" WHERE "Date" >= $month_start',
    );
    expect(canonical.fingerprint).toMatch(/^[0-9a-f]{64}$/);
  });

  it("keeps exact i64 literals in Composition documents and leaves JSON payloads opaque", async () => {
    const document = JSON.parse(JSON.stringify(typedCompositionDocument));
    document.spec.variables.unshift({
      id: "max_i64",
      kind: "input",
      value_type: { scalar_type: "integer", nullable: false },
      required: false,
      default: { kind: "integer", value: "9223372036854775807" },
    });
    document.spec.variables.push({
      id: "json_payload",
      kind: "input",
      value_type: { scalar_type: "json", nullable: false },
      required: false,
      default: {
        kind: "json",
        value: { kind: "integer", value: "9223372036854775807" },
      },
    });
    document.spec.components.push(
      { id: "max_i64_input", kind: "input", variable: "max_i64" },
      { id: "json_payload_input", kind: "input", variable: "json_payload" },
    );
    document.spec.layout.rows[0].items.push(
      { kind: "component", component: "max_i64_input" },
      { kind: "component", component: "json_payload_input" },
    );

    const canonical = await canonicalizeCompositionDocument(document);
    const variables = (canonical.document as typeof document).spec.variables;
    expect(variables[0].default.value).toBe("9223372036854775807");
    expect(
      variables.find((variable: { id: string }) =>
        variable.id === "json_payload"
      ).default.value,
    ).toEqual({
      kind: "integer",
      value: "9223372036854775807",
    });
    expect(canonical.canonical_yaml).toContain("value: 9223372036854775807");
  });

  it("rejects prerelease source-shaped Composition documents", async () => {
    await expect(
      canonicalizeCompositionDocument(
        prereleaseSourceShapedCompositionDocument,
      ),
    ).rejects.toMatchObject({
      kind: "composition_diagnostic",
      code: "invalid_composition",
    });
  });

  it("compiles a typed variable graph through the versioned Rust/WASM action", async () => {
    const integer = { scalar_type: "integer", nullable: false } as const;
    const graph: VariableGraph = {
      variables: [
        {
          id: "total",
          kind: "query",
          returns: { kind: "scalar", value_type: integer },
          query: {
            kind: "inline_sql",
            sql: "SELECT $amount",
            arguments: [{
              name: "amount",
              variable: "amount",
            }],
          },
        },
        {
          id: "amount",
          kind: "input",
          value_type: integer,
          required: true,
        },
        {
          id: "independent",
          kind: "constant",
          value_type: integer,
          value: { kind: "integer", value: "1" },
        },
      ],
    };

    await expect(compileVariableGraph(graph)).resolves.toEqual({
      contract_version: 1,
      topological_order: ["amount", "independent", "total"],
      dependencies: {
        amount: [],
        independent: [],
        total: ["amount"],
      },
    });

    const invalidGraph: VariableGraph = {
      variables: [
        {
          id: "amount",
          kind: "query",
          returns: {
            kind: "table",
            columns: [{ name: "value", value_type: integer }],
          },
          query: { kind: "inline_sql", sql: "SELECT 1" },
        },
        graph.variables[0],
      ],
    };
    await expect(compileVariableGraph(invalidGraph)).rejects.toMatchObject(
      {
        kind: "variable_graph_diagnostics",
        contract_version: 1,
        diagnostics: [{ code: "variable_type_mismatch" }],
      } satisfies Partial<UgoiteApiError>,
    );
  });

  it("parses bounded SQL drafts through the typed Rust/WASM document contract", async () => {
    const sql = '-- 日本語\nSELECT $day FROM "G"';
    const document = await parseSqlDocument(sql);
    const sourceBytes = new TextEncoder().encode(sql);

    expect(document).toMatchObject({
      contract_version: 25,
      source: sql,
      status: "parsed",
    });
    expect(document.tokens.some((token) =>
      token.kind === "placeholder" &&
      new TextDecoder().decode(
          sourceBytes.subarray(token.range.start, token.range.end),
        ) === "$day"
    )).toBe(true);
    expect(document.nodes.some((node) => node.kind === "query_statement"))
      .toBe(true);
    expect(document.nodes.some((node) => node.kind === "select_item"))
      .toBe(true);
    expect(document.nodes.some((node) =>
      node.kind === "relation" &&
      node.semantic?.kind === "relation" &&
      node.semantic.name.some((part) => part.value === "G")
    )).toBe(true);
    expect(document.nodes.some((node) =>
      node.kind === "literal_expression" &&
      node.semantic?.kind === "literal" &&
      node.semantic.literal_kind === "placeholder" &&
      node.semantic.raw === "$day"
    )).toBe(true);
    expect(
      document.nodes.filter((node) => node.kind.endsWith("_expression")).every((
        node,
      ) => node.semantic !== null),
    ).toBe(true);

    await expect(parseSqlDocument("SELECT (")).resolves.toMatchObject({
      source: "SELECT (",
      status: "invalid_sql",
      diagnostics: [{ code: "parse_failed" }],
    });
  });

  it("classifies quoted Form references with Rust CTE scopes", async () => {
    await expect(
      collectSqlFormNameReferences(
        'WITH "Entry" AS (SELECT 1) SELECT * FROM "Entry"',
      ),
    ).resolves.toEqual([]);
    await expect(
      collectSqlFormNameReferences(
        'SELECT * FROM (WITH "Entry" AS (SELECT 1) SELECT * FROM "Entry") AS nested JOIN "Entry" AS actual ON TRUE',
      ),
    ).resolves.toEqual(["Entry"]);
    await expect(collectSqlFormNameReferences("SELECT ("))
      .rejects.toMatchObject({ kind: "domain_validation" });
  });

  it("applies typed SQL Document edits atomically and returns a usable undo", async () => {
    const source = '-- 日本語\nSELECT 1 FROM "G"';
    const start = new TextEncoder().encode(source.slice(0, source.indexOf("1")))
      .length;
    const transaction = {
      expected_source: source,
      edits: [{
        range: { start, end: start + 1 },
        expected_text: "1",
        replacement: "2",
      }],
    };

    const edited = await applySqlDocumentEditTransaction(source, transaction);
    expect(edited.document.source).toBe('-- 日本語\nSELECT 2 FROM "G"');
    expect(edited.document.status).toBe("parsed");

    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("applies Rust-classified SQL Document node edits and returns a usable undo", async () => {
    const source =
      '-- report\nSELECT /* left */ amount AS total FROM "G" WHERE amount > 1 ORDER BY total DESC /* tail */';
    const parsed = await parseSqlDocument(source);
    const predicate = parsed.nodes.find((node) =>
      node.kind === "binary_expression"
    );
    expect(predicate).toBeDefined();

    const edited = await applySqlDocumentNodeEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        node_id: predicate!.id,
        expected_kind: "binary_expression",
        replacement: "amount >= 2",
      }],
    });
    expect(edited.document.source).toBe(
      '-- report\nSELECT /* left */ amount AS total FROM "G" WHERE amount >= 2 ORDER BY total DESC /* tail */',
    );
    expect(edited.document.status).toBe("parsed");

    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("applies typed SQL Document semantic edits through the Rust protocol", async () => {
    const source = '-- 日本語\nSELECT amount FROM "G" WHERE amount > 1';
    const parsed = await parseSqlDocument(source);
    const identifier = parsed.nodes.find((node) =>
      node.kind === "identifier_expression" &&
      node.semantic?.kind === "identifier" &&
      node.semantic.value === "amount"
    );
    const literal = parsed.nodes.find((node) =>
      node.kind === "value" &&
      node.semantic?.kind === "literal" &&
      node.semantic.raw === "1"
    );
    expect(identifier).toBeDefined();
    expect(literal).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [
        {
          kind: "replace_identifier",
          node_id: identifier!.id,
          expected_kind: "identifier_expression",
          parts: [{ value: "total", quote_style: '"' }],
        },
        {
          kind: "replace_literal",
          node_id: literal!.id,
          expected_kind: "value",
          literal: { kind: "number", value: "9007199254740993" },
        },
      ],
    });

    expect(edited.document.source).toBe(
      '-- 日本語\nSELECT "total" FROM "G" WHERE amount > 9007199254740993',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("edits set operators and quantifiers through the Rust protocol", async () => {
    const source = "SELECT 1 UNION ALL SELECT 2";
    const parsed = await parseSqlDocument(source);
    const setOperation = parsed.nodes.find((node) =>
      node.kind === "set_operation" &&
      node.semantic?.kind === "set_operation"
    );
    expect(setOperation).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "replace_set_operation",
        node_id: setOperation!.id,
        expected_kind: "set_operation",
        operator: "except",
        quantifier: "distinct",
      }],
    });

    expect(edited.document.source).toBe("SELECT 1 EXCEPT DISTINCT SELECT 2");
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("swaps set-operation operands through the Rust protocol", async () => {
    const source = "SELECT 1 UNION ALL SELECT 2";
    const parsed = await parseSqlDocument(source);
    const setOperation = parsed.nodes.find((node) =>
      node.kind === "set_operation" &&
      node.semantic?.kind === "set_operation"
    );
    expect(setOperation).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "swap_set_operation_operands",
        node_id: setOperation!.id,
        expected_kind: "set_operation",
      }],
    });

    expect(edited.document.source).toBe("SELECT 2 UNION ALL SELECT 1");
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("edits CAST target types through the Rust protocol", async () => {
    const source = "SELECT CAST(amount + 1 AS BIGINT)";
    const parsed = await parseSqlDocument(source);
    const cast = parsed.nodes.find((node) =>
      node.kind === "cast_expression" && node.semantic?.kind === "cast"
    );
    expect(cast).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "replace_cast_target_type",
        node_id: cast!.id,
        expected_kind: "cast_expression",
        target_type: "double",
      }],
    });

    expect(edited.document.source).toBe("SELECT CAST(amount + 1 AS DOUBLE)");
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("wraps a typed expression in CAST through the Rust protocol", async () => {
    const source = 'SELECT amount + 1 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const expression = parsed.nodes.find((node) =>
      node.kind === "binary_expression"
    );
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "wrap_expression_in_cast",
        node_id: expression!.id,
        expected_kind: "binary_expression",
        target_type: "double",
      }],
    });

    expect(edited.document.source).toBe(
      'SELECT CAST(amount + 1 AS DOUBLE) FROM "G"',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("builds registered aggregate expressions through the Rust protocol", async () => {
    const source = 'SELECT amount + 1 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const expression = parsed.nodes.find((node) =>
      node.kind === "binary_expression"
    );
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "wrap_expression_in_aggregate",
        node_id: expression!.id,
        expected_kind: "binary_expression",
        function: "sum",
        distinct: true,
      }],
    });

    expect(edited.document.source).toBe(
      'SELECT SUM(DISTINCT amount + 1) FROM "G"',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("wraps a typed expression in an editable searched CASE in Rust", async () => {
    const source = 'SELECT amount + 1 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const expression = parsed.nodes.find((node) =>
      node.kind === "binary_expression"
    );
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "wrap_expression_in_case",
        node_id: expression!.id,
        expected_kind: "binary_expression",
      }],
    });

    expect(edited.document.source).toBe(
      'SELECT CASE WHEN TRUE THEN amount + 1 ELSE NULL END FROM "G"',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("adds WHERE before GROUP BY, ORDER BY, and LIMIT through Rust", async () => {
    const source = 'SELECT 1 FROM "G" GROUP BY 1 ORDER BY 1 LIMIT 2';
    const parsed = await parseSqlDocument(source);
    const select = parsed.nodes.find((node) => node.kind === "select");
    expect(select).toBeDefined();
    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "add_where_clause",
        node_id: select!.id,
        expected_kind: "select",
      }],
    });
    expect(edited.document.source).toBe(
      'SELECT 1 FROM "G" WHERE TRUE GROUP BY 1 ORDER BY 1 LIMIT 2',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("builds BETWEEN with independently editable bounds through Rust", async () => {
    const source = 'SELECT amount + 1 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const expression = parsed.nodes.find((node) =>
      node.kind === "binary_expression"
    );
    expect(expression).toBeDefined();
    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "wrap_expression_in_predicate",
        node_id: expression!.id,
        expected_kind: "binary_expression",
        template: "between",
      }],
    });
    expect(edited.document.source).toBe(
      'SELECT (amount + 1) BETWEEN NULL AND NULL FROM "G"',
    );
    expect(
      edited.document.nodes.filter((node) => node.kind === "literal_expression")
        .length,
    ).toBeGreaterThanOrEqual(2);
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("appends and removes typed values in IN lists through Rust", async () => {
    const source = 'SELECT amount FROM "G" WHERE amount IN (1, 2) /* keep */';
    const parsed = await parseSqlDocument(source);
    const predicate = parsed.nodes.find((node) =>
      node.kind === "predicate_expression" &&
      node.semantic?.kind === "predicate" &&
      node.semantic.operator === "in"
    );
    expect(predicate).toBeDefined();

    const appended = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "append_in_list_value",
        node_id: predicate!.id,
        expected_kind: "predicate_expression",
      }],
    });
    expect(appended.document.source).toBe(
      'SELECT amount FROM "G" WHERE amount IN (1, 2, NULL) /* keep */',
    );

    const appendedPredicate = appended.document.nodes.find((node) =>
      node.kind === "predicate_expression" &&
      node.semantic?.kind === "predicate" &&
      node.semantic.operator === "in"
    );
    expect(appendedPredicate).toBeDefined();
    const removed = await applySqlDocumentSemanticEditTransaction(
      appended.document.source,
      {
        contract_version: appended.document.contract_version,
        expected_source: appended.document.source,
        edits: [{
          kind: "remove_last_in_list_value",
          node_id: appendedPredicate!.id,
          expected_kind: "predicate_expression",
        }],
      },
    );
    expect(removed.document.source).toBe(source);
    const restored = await applySqlDocumentEditTransaction(
      removed.document.source,
      removed.undo,
    );
    expect(restored.document.source).toBe(appended.document.source);
  });

  it("adds and removes CASE branches through the Rust protocol", async () => {
    const source = "SELECT CASE WHEN amount > 0 THEN 1 ELSE 0 END";
    const parsed = await parseSqlDocument(source);
    const caseNode = parsed.nodes.find((node) =>
      node.kind === "case_expression" && node.semantic?.kind === "case"
    );
    expect(caseNode).toBeDefined();

    const added = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "add_case_branch",
        node_id: caseNode!.id,
        expected_kind: "case_expression",
      }],
    });
    expect(added.document.source).toBe(
      "SELECT CASE WHEN amount > 0 THEN 1 WHEN TRUE THEN NULL ELSE 0 END",
    );

    const addedCase = added.document.nodes.find((node) =>
      node.kind === "case_expression" && node.semantic?.kind === "case"
    );
    expect(addedCase?.semantic).toMatchObject({ kind: "case", when_count: 2 });
    const removed = await applySqlDocumentSemanticEditTransaction(
      added.document.source,
      {
        contract_version: added.document.contract_version,
        expected_source: added.document.source,
        edits: [{
          kind: "remove_case_branch",
          node_id: addedCase!.id,
          expected_kind: "case_expression",
        }],
      },
    );
    expect(removed.document.source).toBe(source);
    const restored = await applySqlDocumentEditTransaction(
      added.document.source,
      added.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("toggles CASE ELSE through the Rust protocol", async () => {
    const source = "SELECT CASE WHEN amount > 0 THEN 1 END";
    const parsed = await parseSqlDocument(source);
    const caseNode = parsed.nodes.find((node) =>
      node.kind === "case_expression" && node.semantic?.kind === "case"
    );
    expect(caseNode).toBeDefined();

    const added = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "set_case_else",
        node_id: caseNode!.id,
        expected_kind: "case_expression",
        has_else: true,
      }],
    });
    expect(added.document.source).toBe(
      "SELECT CASE WHEN amount > 0 THEN 1 ELSE NULL END",
    );
  });

  it("edits aggregate arguments from typed expression nodes through Rust", async () => {
    const source = 'SELECT COUNT(field_100) AS item_count, field_100 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const count = parsed.nodes.find((node) =>
      node.kind === "function_expression" &&
      node.semantic?.kind === "function" &&
      node.semantic.name.at(-1)?.value.toLowerCase() === "count"
    );
    const argument = parsed.nodes.find((node) =>
      node.kind === "identifier_expression" &&
      node.parent === count?.id &&
      node.semantic?.kind === "identifier" &&
      node.semantic.value === "field_100"
    );
    expect(count?.semantic).toMatchObject({
      kind: "function",
      argument_count: 1,
      can_replace_argument: true,
      can_add_argument: true,
      can_remove_argument: true,
    });
    expect(argument).toBeDefined();

    const appended = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "set_function_argument",
        node_id: count!.id,
        expected_kind: "function_expression",
        argument_index: 1,
        expression_node_id: argument!.id,
        expression_expected_kind: "identifier_expression",
      }],
    });
    expect(appended.document.source).toBe(
      'SELECT COUNT(field_100, field_100) AS item_count, field_100 FROM "G"',
    );

    const editedFunction = appended.document.nodes.find((node) =>
      node.kind === "function_expression" &&
      node.semantic?.kind === "function"
    );
    const removed = await applySqlDocumentSemanticEditTransaction(
      appended.document.source,
      {
        contract_version: appended.document.contract_version,
        expected_source: appended.document.source,
        edits: [{
          kind: "remove_last_function_argument",
          node_id: editedFunction!.id,
          expected_kind: "function_expression",
        }],
      },
    );
    expect(removed.document.source).toBe(source);
    const restored = await applySqlDocumentEditTransaction(
      appended.document.source,
      appended.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("replaces a SELECT expression with another typed tree expression", async () => {
    const source =
      "SELECT field_100, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END AS label FROM \"G\"";
    const parsed = await parseSqlDocument(source);
    const items = parsed.nodes.filter((node) => node.kind === "select_item")
      .sort((left, right) =>
        (left.range?.start ?? 0) - (right.range?.start ?? 0)
      );
    const expression = parsed.nodes.find((node) =>
      node.parent === items[1]?.id && node.kind === "case_expression"
    );
    expect(items).toHaveLength(2);
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "set_projection_expression",
        node_id: items[0].id,
        expected_kind: "select_item",
        expression_node_id: expression!.id,
        expression_expected_kind: "case_expression",
      }],
    });
    expect(edited.document.source).toBe(
      "SELECT CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END, CASE WHEN field_100 = 'alpha' THEN field_100 ELSE 'other' END AS label FROM \"G\"",
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("composes a SELECT expression from typed SQL nodes in Rust", async () => {
    const source = `SELECT field_100, 'x' FROM "G"`;
    const parsed = await parseSqlDocument(source);
    const items = parsed.nodes.filter((node) => node.kind === "select_item")
      .sort((left, right) =>
        (left.range?.start ?? 0) - (right.range?.start ?? 0)
      );
    const left = parsed.nodes.find((node) =>
      node.parent === items[0]?.id && node.kind === "identifier_expression"
    );
    const right = parsed.nodes.find((node) =>
      node.semantic?.kind === "literal" && node.semantic.raw === "'x'"
    );
    expect(left).toBeDefined();
    expect(right).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "compose_binary_expression",
        node_id: items[0].id,
        expected_kind: "select_item",
        left_expression_node_id: left!.id,
        left_expression_expected_kind: left!.kind,
        operator: "concat",
        right_expression_node_id: right!.id,
        right_expression_expected_kind: right!.kind,
      }],
    });
    expect(edited.document.source).toBe(
      `SELECT (field_100\n) || ('x'\n), 'x' FROM "G"`,
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("composes a non-projection expression from typed SQL nodes in Rust", async () => {
    const source = "SELECT 1 WHERE 1 = 2";
    const parsed = await parseSqlDocument(source);
    const whereOffset = source.indexOf("WHERE");
    const values = parsed.nodes.filter((node) =>
      node.kind === "value" && node.range
    );
    const projectionValue = values.find((node) =>
      node.range!.start < whereOffset &&
      source.slice(node.range!.start, node.range!.end) === "1"
    );
    const whereLeft = values.find((node) =>
      node.range!.start > whereOffset &&
      source.slice(node.range!.start, node.range!.end) === "1"
    );
    const whereRight = values.find((node) =>
      node.range!.start > whereOffset &&
      source.slice(node.range!.start, node.range!.end) === "2"
    );
    expect(projectionValue).toBeDefined();
    expect(whereLeft).toBeDefined();
    expect(whereRight).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "compose_binary_expression",
        node_id: whereLeft!.id,
        expected_kind: whereLeft!.kind,
        left_expression_node_id: projectionValue!.id,
        left_expression_expected_kind: projectionValue!.kind,
        operator: "add",
        right_expression_node_id: whereRight!.id,
        right_expression_expected_kind: whereRight!.kind,
      }],
    });
    expect(edited.document.source).toBe(
      "SELECT 1 WHERE (1\n) + (2\n) = 2",
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("adds a typed expression to GROUP BY in Rust", async () => {
    const source = `SELECT field_100 FROM "G" ORDER BY field_100`;
    const parsed = await parseSqlDocument(source);
    const select = parsed.nodes.find((node) => node.kind === "select");
    const expression = parsed.nodes.find((node) =>
      node.kind === "identifier_expression" &&
      node.parent !== null &&
      parsed.nodes[node.parent]?.kind === "select_item"
    );
    expect(select).toBeDefined();
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "add_group_by_expression",
        node_id: select!.id,
        expected_kind: "select",
        expression_node_id: expression!.id,
        expression_expected_kind: expression!.kind,
      }],
    });
    expect(edited.document.source).toBe(
      `SELECT field_100 FROM "G" GROUP BY field_100 ORDER BY field_100`,
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("adds a typed expression to ORDER BY before the row limit in Rust", async () => {
    const source = 'SELECT field_100 FROM "G" LIMIT 10';
    const parsed = await parseSqlDocument(source);
    const query = parsed.nodes.find((node) => node.kind === "query");
    const expression = parsed.nodes.find((node) =>
      node.kind === "identifier_expression" &&
      node.parent !== null &&
      parsed.nodes[node.parent]?.kind === "select_item"
    );
    expect(query).toBeDefined();
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "add_order_by_expression",
        node_id: query!.id,
        expected_kind: "query",
        expression_node_id: expression!.id,
        expression_expected_kind: expression!.kind,
        direction: "desc",
        nulls_order: "last",
      }],
    });
    expect(edited.document.source).toBe(
      'SELECT field_100 FROM "G" ORDER BY field_100 DESC NULLS LAST LIMIT 10',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("creates a literal leaf from an existing typed expression in Rust", async () => {
    const source = 'SELECT field_100 + 1 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const expression = parsed.nodes.find((node) =>
      node.kind === "binary_expression"
    );
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "replace_literal",
        node_id: expression!.id,
        expected_kind: "binary_expression",
        literal: { kind: "number", value: "7" },
      }],
    });
    expect(edited.document.source).toBe('SELECT 7 FROM "G"');
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("replaces any expression from another typed node in the same query", async () => {
    const source = 'SELECT field_100 + 1, field_100 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const target = parsed.nodes.find((node) =>
      node.kind === "binary_expression" &&
      node.range &&
      source.slice(node.range.start, node.range.end) === "field_100 + 1"
    );
    const expression = parsed.nodes.find((node) =>
      node.kind === "identifier_expression" &&
      node.range && node.range.start > (target?.range?.end ?? 0)
    );
    expect(target).toBeDefined();
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "replace_expression_from_node",
        node_id: target!.id,
        expected_kind: "binary_expression",
        expression_node_id: expression!.id,
        expression_expected_kind: "identifier_expression",
      }],
    });
    expect(edited.document.source).toBe(
      'SELECT field_100, field_100 FROM "G"',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("adds a SELECT expression from another typed node in Rust", async () => {
    const source = 'SELECT field_100 FROM "G" WHERE field_100 > 10';
    const parsed = await parseSqlDocument(source);
    const list = parsed.nodes.find((node) => node.kind === "select_list");
    const expression = parsed.nodes.find((node) =>
      node.kind === "binary_expression"
    );
    expect(list).toBeDefined();
    expect(expression).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "add_projection_expression",
        node_id: list!.id,
        expected_kind: "select_list",
        expression_node_id: expression!.id,
        expression_expected_kind: "binary_expression",
      }],
    });
    expect(edited.document.source).toBe(
      'SELECT field_100, field_100 > 10 FROM "G" WHERE field_100 > 10',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("adds a relation-free scalar subquery projection through Rust", async () => {
    const source = 'SELECT 1 FROM "G"';
    const parsed = await parseSqlDocument(source);
    const list = parsed.nodes.find((node) => node.kind === "select_list");
    expect(list).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "add_scalar_subquery_projection",
        node_id: list!.id,
        expected_kind: "select_list",
      }],
    });
    expect(edited.document.source).toBe(
      'SELECT 1, (SELECT 1) AS __ugoite_scalar_subquery FROM "G"',
    );
    expect(
      edited.document.nodes.some((node) => node.kind === "subquery_expression"),
    ).toBe(true);
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("renames a CTE and its relation references through the Rust protocol", async () => {
    const source = "WITH c AS (SELECT 1 AS n) SELECT n FROM c";
    const parsed = await parseSqlDocument(source);
    const cte = parsed.nodes.find((node) =>
      node.kind === "common_table_expression" &&
      node.semantic?.kind === "common_table_expression"
    );
    expect(cte).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "rename_common_table_expression",
        node_id: cte!.id,
        expected_kind: "common_table_expression",
        name: { value: "rows", quote_style: '"' },
      }],
    });

    expect(edited.document.source).toBe(
      'WITH "rows" AS (SELECT 1 AS n) SELECT n FROM "rows"',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("respects nested CTE shadowing through the Rust protocol", async () => {
    const source =
      "WITH c AS (SELECT 1 AS n), d AS (WITH c AS (SELECT 2 AS n) SELECT n FROM c), e AS (SELECT n FROM c) SELECT n FROM c";
    const parsed = await parseSqlDocument(source);
    const cte = parsed.nodes.find((node) =>
      node.kind === "common_table_expression" &&
      node.semantic?.kind === "common_table_expression" &&
      node.semantic.name.value === "c"
    );
    expect(cte).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "rename_common_table_expression",
        node_id: cte!.id,
        expected_kind: "common_table_expression",
        name: { value: "rows", quote_style: null },
      }],
    });

    expect(edited.document.source).toBe(
      "WITH rows AS (SELECT 1 AS n), d AS (WITH c AS (SELECT 2 AS n) SELECT n FROM c), e AS (SELECT n FROM rows) SELECT n FROM rows",
    );
  });

  it("toggles predicate negation through the Rust protocol and preserves comments", async () => {
    const source = 'SELECT amount FROM "G" WHERE amount IS /* keep */ NULL';
    const parsed = await parseSqlDocument(source);
    const predicate = parsed.nodes.find((node) =>
      node.kind === "predicate_expression" &&
      node.semantic?.kind === "predicate" &&
      node.semantic.operator === "is_null"
    );
    expect(predicate).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "replace_predicate_negation",
        node_id: predicate!.id,
        expected_kind: "predicate_expression",
        negated: true,
      }],
    });

    expect(edited.document.source).toBe(
      'SELECT amount FROM "G" WHERE amount IS NOT /* keep */ NULL',
    );
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("edits arithmetic unary signs through the Rust protocol and preserves comments", async () => {
    const source = "SELECT - /* keep */ amount";
    const parsed = await parseSqlDocument(source);
    const unary = parsed.nodes.find((node) =>
      node.kind === "unary_expression" &&
      node.semantic?.kind === "unary_operator" &&
      node.semantic.operator === "-"
    );
    expect(unary).toBeDefined();

    const edited = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "replace_unary_sign",
        node_id: unary!.id,
        expected_kind: "unary_expression",
        sign: "positive",
      }],
    });

    expect(edited.document.source).toBe("SELECT + /* keep */ amount");
    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("initializes empty SQL through the actual WASM transaction and restores exact trivia", async () => {
    const source = "-- 日本語";
    const parsed = await parseSqlDocument(source);
    expect(parsed.status).toBe("empty");
    const result = await applySqlDocumentSemanticEditTransaction(source, {
      contract_version: parsed.contract_version,
      expected_source: source,
      edits: [{
        kind: "initialize_select",
        relation: { value: 'Travel "Costs"', quote_style: '"' },
      }],
    });
    expect(result.document.source).toBe(
      '-- 日本語\nSELECT * FROM "Travel ""Costs"""',
    );
    expect(result.document.status).toBe("parsed");
    expect(
      (await applySqlDocumentEditTransaction(
        result.document.source,
        result.undo,
      )).document.source,
    ).toBe(source);
    await expect(applySqlDocumentSemanticEditTransaction("SELECT (", {
      contract_version: parsed.contract_version,
      expected_source: "SELECT (",
      edits: [{ kind: "initialize_select", relation: null }],
    })).rejects.toMatchObject({
      kind: "sql_document_edit",
      code: "invalid_semantic_replacement",
    });
  });

  it("reports typed SQL Document node edit failures with stable codes", async () => {
    const source = 'SELECT amount FROM "G"';
    const parsed = await parseSqlDocument(source);
    const projection = parsed.nodes.find((node) => node.kind === "select_item");
    expect(projection).toBeDefined();

    await expect(
      applySqlDocumentNodeEditTransaction(source, {
        contract_version: parsed.contract_version + 1,
        expected_source: source,
        edits: [],
      }),
    ).rejects.toMatchObject({
      kind: "sql_document_edit",
      code: "contract_version_mismatch",
    });

    await expect(
      applySqlDocumentNodeEditTransaction(source, {
        contract_version: parsed.contract_version,
        expected_source: source,
        edits: [{
          node_id: projection!.id,
          expected_kind: "relation",
          replacement: "other",
        }],
      }),
    ).rejects.toMatchObject({
      kind: "sql_document_edit",
      code: "node_kind_mismatch",
    });
  });

  it("keeps incomplete SQL Document edit drafts and reports stable stale/range failures", async () => {
    const source = "SELECT 1";
    const draft = await applySqlDocumentEditTransaction(source, {
      expected_source: source,
      edits: [{
        range: { start: 7, end: 8 },
        expected_text: "1",
        replacement: "(",
      }],
    });
    expect(draft.document).toMatchObject({
      source: "SELECT (",
      status: "invalid_sql",
      diagnostics: [{ code: "parse_failed" }],
    });

    await expect(
      applySqlDocumentEditTransaction("SELECT 2", {
        expected_source: source,
        edits: [],
      }),
    ).rejects.toMatchObject({
      kind: "sql_document_edit",
      code: "stale_source",
    });

    await expect(
      applySqlDocumentEditTransaction(source, {
        expected_source: source,
        edits: [{
          range: { start: 8, end: 9 },
          expected_text: "1",
          replacement: "2",
        }],
      }),
    ).rejects.toMatchObject({
      kind: "sql_document_edit",
      code: "range_out_of_bounds",
    });
  });

  it("exposes SQL Document edit codes through the typed API error contract", () => {
    const sqlEditError = new UgoiteApiError<SqlDocumentEditErrorCode>({
      kind: "sql_document_edit",
      message: "The SQL source changed before the edit was applied",
      code: "stale_source",
    });
    const sqlEditCode: SqlDocumentEditErrorCode | undefined = sqlEditError.code;
    expect(sqlEditCode).toBe("stale_source");

    const unrelatedError = new UgoiteApiError({
      kind: "not_found",
      message: "Space was not found",
      code: "space_missing",
    });
    const unrelatedApiCode: string | undefined = unrelatedError.code;
    expect(unrelatedApiCode).toBe("space_missing");
  });

  it("bounds SQL Document edit result serialization", async () => {
    const source = `SELECT ${Array(20_000).fill("1").join(",")}`;
    await expect(
      applySqlDocumentEditTransaction(source, {
        expected_source: source,
        edits: [],
      }),
    ).rejects.toMatchObject({ kind: "domain_output_too_large" });
  }, 30_000);

  it("parses graph Input with the shared Rust Domain contract without coercion", async () => {
    const cases = [
      [{ scalar_type: "string", nullable: false }, "", { status: "unset" }],
      [{ scalar_type: "string", nullable: false }, "日本語", {
        status: "valid",
        value: "日本語",
      }],
      [{ scalar_type: "string", nullable: false }, "x".repeat(4097), {
        status: "invalid",
      }],
      [{ scalar_type: "boolean", nullable: false }, "false", {
        status: "valid",
        value: false,
      }],
      [{ scalar_type: "boolean", nullable: false }, "FALSE", {
        status: "invalid",
      }],
      [{ scalar_type: "integer", nullable: false }, "9007199254740991", {
        status: "valid",
        value: "9007199254740991",
      }],
      [{ scalar_type: "integer", nullable: false }, "9007199254740993", {
        status: "valid",
        value: "9007199254740993",
      }],
      [{ scalar_type: "integer", nullable: false }, "-9223372036854775808", {
        status: "valid",
        value: "-9223372036854775808",
      }],
      [{ scalar_type: "integer", nullable: false }, "9223372036854775807", {
        status: "valid",
        value: "9223372036854775807",
      }],
      [{ scalar_type: "integer", nullable: false }, "1.0", {
        status: "invalid",
      }],
      [{ scalar_type: "float", nullable: false }, "1", {
        status: "valid",
        value: 1,
      }],
      [{ scalar_type: "float", nullable: false }, "1e-4000", {
        status: "invalid",
      }],
      [{ scalar_type: "float", nullable: false }, "1e309", {
        status: "invalid",
      }],
      [{ scalar_type: "decimal", nullable: false }, "12.3400", {
        status: "valid",
        value: "12.3400",
      }],
      [{ scalar_type: "decimal", nullable: false }, "1e3", {
        status: "invalid",
      }],
      [{ scalar_type: "date", nullable: false }, "2024-02-29", {
        status: "valid",
        value: "2024-02-29",
      }],
      [{ scalar_type: "date", nullable: false }, "2025-02-29", {
        status: "invalid",
      }],
      [
        { scalar_type: "timestamp", nullable: false },
        "2026-10-13T23:59:59.123456789+09:00",
        {
          status: "valid",
          value: "2026-10-13T23:59:59.123456789+09:00",
        },
      ],
      [{ scalar_type: "timestamp", nullable: false }, "2026-10-13T14:59:59", {
        status: "invalid",
      }],
      [{ scalar_type: "json", nullable: false }, '{"n":1}', {
        status: "valid",
        value: { n: 1 },
      }],
      [{ scalar_type: "json", nullable: true }, "null", { status: "invalid" }],
    ] as const;

    for (const [valueType, raw, expected] of cases) {
      await expect(parseVariableGraphInput(valueType, raw)).resolves.toEqual(
        expected,
      );
    }
  });

  it("renames SQL Document parameters through the typed Rust/WASM client", async () => {
    const sql = `-- $day
SELECT $day + $day AS total, '$day' AS literal, $day2 AS other FROM "G"`;
    const document = await renameSqlDocumentParameter(
      sql,
      "day",
      "billing_day",
    );
    const expectedSql = sql.replace(
      "SELECT $day + $day",
      "SELECT $billing_day + $billing_day",
    );
    const sourceBytes = new TextEncoder().encode(expectedSql);

    expect(document).toMatchObject({ source: expectedSql, status: "parsed" });
    expect(
      document.tokens.filter((token) => token.kind === "placeholder")
        .map((token) =>
          new TextDecoder().decode(
            sourceBytes.subarray(token.range.start, token.range.end),
          )
        ),
    ).toEqual(["$billing_day", "$billing_day", "$day2"]);
  });

  it("parses graph Input synchronously after preparing the shared WASM parser", async () => {
    await prepareVariableGraphInputParser();

    expect(
      parseVariableGraphInputSync(
        { scalar_type: "integer", nullable: false },
        "+00042",
      ),
    ).toEqual({
      status: "valid",
      value: "42",
    });
    expect(
      parseVariableGraphInputSync(
        { scalar_type: "date", nullable: false },
        "2025-02-29",
      ),
    ).toEqual({
      status: "invalid",
    });
    expect(
      parseVariableGraphInputSync(
        { scalar_type: "string", nullable: false },
        "",
      ),
    ).toEqual({
      status: "unset",
    });
  });

  it("preserves typed Composition i64 values across real HTTP and WASM boundaries", async () => {
    const responseBody =
      '{"evaluation_id":"eval-1","ok":true,"computed_query_ids":[],"variable_values":{"max":{"kind":"integer","value":9223372036854775807},"min":{"kind":"integer","value":-9223372036854775808}},"queries":[],"component_bindings":[],"dependencies":{},"query_results":{"rows":{"value":{"kind":"table","columns":[{"name":"max","value_type":{"scalar_type":"integer","nullable":false}},{"name":"json","value_type":{"scalar_type":"json","nullable":false}}],"rows":[[{"kind":"integer","value":9223372036854775807},{"kind":"json","value":{"kind":"integer","value":7}}]]},"has_more":false}}}';
    server.use(
      http.post(
        testApiUrl("/spaces/demo/compositions/evaluate"),
        async ({ request }) => {
          const body = await request.text();
          expect(body).toContain('"max":9223372036854775807');
          expect(body).toContain('"min":-9223372036854775808');
          return new Response(responseBody, {
            headers: { "content-type": "application/json" },
          });
        },
      ),
    );

    const response = await compositionApi.evaluate(
      "demo",
      {
        evaluation_id: "eval-1",
        target: { kind: "draft", yaml: "format: ugoite.composition" },
        variables: {
          max: "9223372036854775807",
          min: "-9223372036854775808",
        },
      },
      undefined,
      ["max", "min"],
    );

    expect(response.variable_values.max).toEqual({
      kind: "integer",
      value: "9223372036854775807",
    });
    expect(response.variable_values.min).toEqual({
      kind: "integer",
      value: "-9223372036854775808",
    });
    expect(response.query_results.rows.value.kind).toBe("table");
    if (response.query_results.rows.value.kind === "table") {
      expect(response.query_results.rows.value.rows[0][0]).toEqual({
        kind: "integer",
        value: "9223372036854775807",
      });
      expect(response.query_results.rows.value.rows[0][1]).toEqual({
        kind: "json",
        value: { kind: "integer", value: 7 },
      });
    }
  });

  it("reopens a canonical Composition integer from the raw lint response", async () => {
    const responseBody =
      '{"ok":true,"value":{"document":{"format":"ugoite.composition","format_version":1,"kind":"dashboard","name":"Report","spec":{"variables":[{"id":"limit","kind":"input","value_type":{"scalar_type":"integer","nullable":false},"required":false,"default":{"kind":"integer","value":9007199254740993}}],"components":[],"layout":{"kind":"flow","rows":[]}}},"canonical_yaml":"format: ugoite.composition","fingerprint":"fingerprint"}}';
    server.use(
      http.post(
        testApiUrl("/compositions/lint"),
        () =>
          new Response(responseBody, {
            headers: { "content-type": "application/json" },
          }),
      ),
    );
    const reopened = await protocolFetch<{
      ok: boolean;
      value: {
        document: {
          spec: { variables: Array<{ default: { value: string } }> };
        };
      };
    }>("composition.lint", {}, { yaml: "format: ugoite.composition" });

    expect(reopened.value.document.spec.variables[0].default.value).toBe(
      "9007199254740993",
    );
  });

  it("decodes typed i64 defaults from Composition preview and leaves JSON defaults opaque", async () => {
    const responseBody =
      '{"ok":false,"draft_fingerprint":"draft","variable_definitions":[{"id":"max","value_type":{"scalar_type":"integer","nullable":false},"required":false,"default":{"kind":"integer","value":9223372036854775807}},{"id":"json_payload","value_type":{"scalar_type":"json","nullable":false},"required":false,"default":{"kind":"json","value":{"kind":"integer","value":9223372036854775807}}}],"plan":null,"diagnostics":[]}';
    server.use(
      http.post(
        testApiUrl("/spaces/demo/compositions/preview"),
        () =>
          new Response(responseBody, {
            headers: { "content-type": "application/json" },
          }),
      ),
    );

    const preview = await protocolFetch<{
      variable_definitions: Array<{
        id: string;
        default?: { kind: string; value: unknown };
      }>;
    }>("composition.preview", { space_id: "demo" }, {
      yaml: "format: ugoite.composition",
      variables: {},
    });

    expect(preview.variable_definitions[0].default?.value).toBe(
      "9223372036854775807",
    );
    expect(preview.variable_definitions[1].default?.value).toEqual({
      kind: "integer",
      value: 9223372036854776000,
    });
  });

  it("rejects out-of-range integer tokens in Composition preview defaults", async () => {
    const responseBody =
      '{"ok":false,"draft_fingerprint":"draft","variable_definitions":[{"id":"too_large","value_type":{"scalar_type":"integer","nullable":false},"required":false,"default":{"kind":"integer","value":9223372036854775808}}],"plan":null,"diagnostics":[]}';
    server.use(
      http.post(
        testApiUrl("/spaces/demo/compositions/preview"),
        () =>
          new Response(responseBody, {
            headers: { "content-type": "application/json" },
          }),
      ),
    );

    await expect(
      protocolFetch("composition.preview", { space_id: "demo" }, {
        yaml: "format: ugoite.composition",
        variables: {},
      }),
    ).rejects.toThrow("Protocol integer is outside the supported i64 range");
  });

  it("does not treat a non-document lint value as a Composition document", async () => {
    server.use(
      http.post(
        testApiUrl("/compositions/lint"),
        () =>
          HttpResponse.json({
            value: null,
            canonical_yaml: "format: ugoite.composition",
          }),
      ),
    );

    await expect(
      protocolFetch("composition.lint", {}, { yaml: "invalid" }),
    ).resolves.toMatchObject({ value: null });
  });

  it("rejects unsafe JavaScript numbers instead of encoding their rounded value", () => {
    expect(() => exactIntegerJsonValue(9_007_199_254_740_993)).toThrow(
      "Unsafe integer number cannot be encoded exactly",
    );
  });

  it("uses the shared spreadsheet-safe CSV encoder", async () => {
    await expect(
      encodeSpreadsheetCsv([["=SUM(A1:A2)", "a,b", "line\nbreak", "日本語"]]),
    ).resolves.toBe(
      '"\'=SUM(A1:A2)","a,b","line\nbreak","日本語"',
    );
  });

  it("measures the exact CSV request envelope without a guessed margin", () => {
    const rows = [["a", "日本語"]];
    const expected = new TextEncoder().encode(
      JSON.stringify({
        action: "domain.encode_spreadsheet_csv",
        value: rows,
      }),
    ).length;
    expect(buildSpreadsheetCsvRequest(rows)).toEqual({
      action: "domain.encode_spreadsheet_csv",
      value: rows,
    });
    expect(spreadsheetCsvRequestBytes(rows)).toBe(expected);
    expect(spreadsheetCsvRequestBytes(rows)).toBeLessThanOrEqual(256 * 1024);
  });

  it("uses the Rust domain contract for AssetReference validation", async () => {
    await expect(
      validateAssetReference({ ...validReference, asset_id: "../asset" }),
    ).rejects.toThrow();
    await expect(validateAssetReference(validReference)).resolves.toEqual(
      validReference,
    );
  });

  it("REQ-API-001: exposes the expected ABI version and operation manifest", async () => {
    expect(UGOITE_WASM_PROTOCOL_VERSION).toBe(1);
    await expect(getWasmSupportedOperations()).resolves.toEqual(
      UGOITE_API_OPERATIONS,
    );
  });

  it("REQ-API-001: encodes path segments for canonical entry reads", async () => {
    const request = await prepareApiRequest("entry.get", {
      space_id: "team/東京",
      entry_id: "entry-1",
    });

    expect(request).toMatchObject({
      method: "GET",
      path: "/spaces/team%2F%E6%9D%B1%E4%BA%AC/entries/entry-1",
      body_kind: "none",
    });
  });

  it("REQ-API-001: encodes durable collection pagination arguments", async () => {
    await expect(
      prepareApiRequest(
        "entry.query",
        { space_id: "demo" },
        {
          query: { scope: { kind: "all" }, filters: [], sort: [] },
          projection: { kind: "preview" },
          limit: 51,
        },
      ),
    ).resolves.toMatchObject({
      method: "POST",
      path: "/spaces/demo/entries/query",
      body_kind: "json",
    });

    await expect(
      prepareApiRequest("entry.history", {
        space_id: "demo",
        entry_id: "entry-1",
        limit: 51,
        offset: 50,
      }),
    ).resolves.toMatchObject({
      path: "/spaces/demo/entries/entry-1/history?limit=51&offset=50",
    });
  });

  it("REQ-FE-065: sends row-reference lookups through canonical entry.query", async () => {
    const request = await prepareApiRequest(
      "entry.query",
      { space_id: "01900000-0000-7000-8000-000000000001" },
      {
        query: {
          scope: {
            kind: "form",
            form_id: "01900000-0000-7000-8000-000000000002",
          },
          text: "alpha",
          filters: [],
          sort: [],
        },
        projection: { kind: "preview" },
        limit: 8,
      },
    );

    expect(request.path).toBe(
      "/spaces/01900000-0000-7000-8000-000000000001/entries/query",
    );
    expect(request.body_kind).toBe("json");
  });

  it("REQ-API-001: serializes JSON bodies in the portable Rust layer", async () => {
    const request = await prepareApiRequest(
      "entry.create",
      { space_id: "demo" },
      { id: "entry-1", form: "Note", fields: { Body: "Hello" } },
    );

    expect(request.method).toBe("POST");
    expect(request.body_kind).toBe("json");
    expect(request.headers).toContainEqual({
      name: "content-type",
      value: "application/json",
    });
    expect(JSON.parse(request.body ?? "null")).toEqual({
      id: "entry-1",
      form: "Note",
      fields: { Body: "Hello" },
    });
  });

  it("test_req_sec_012_owner_recovery_protocol_forwards_header", async () => {
    const forceReset = await prepareApiRequest(
      "space.recovery.force_reset",
      { space_id: "team" },
      { principal_id: "01900000-0000-7000-8000-000000000001" },
    );
    expect(forceReset).toMatchObject({
      method: "POST",
      path: "/spaces/team/admin/recovery/force-reset",
      body_kind: "json",
    });
    expect(forceReset.headers).toContainEqual({
      name: "content-type",
      value: "application/json",
    });
    expect(forceReset.headers).not.toContainEqual({
      name: "idempotency-key",
      value: expect.any(String),
    });

    const ownerStart = await prepareApiRequest(
      "auth.recovery.owner_start",
      {},
      { owner_approval_token: "owner-token" },
    );
    expect(ownerStart).toMatchObject({
      method: "POST",
      path: "/auth/recovery/owner/start",
    });

    const ownerFinish = await prepareApiRequest(
      "auth.recovery.owner_finish",
      {},
      {
        challenge_id: "01900000-0000-7000-8000-000000000002",
        credential: { id: "credential" },
      },
    );
    expect(ownerFinish).toMatchObject({
      method: "POST",
      path: "/auth/recovery/owner/finish",
    });
    expect(UGOITE_API_OPERATIONS).toEqual(
      expect.arrayContaining([
        "space.recovery.force_reset",
        "auth.recovery.owner_start",
        "auth.recovery.owner_finish",
      ]),
    );
  });

  it("REQ-API-001: executes JSON protocol operations through the browser adapter", async () => {
    server.use(
      http.get(testApiUrl("/spaces/demo"), ({ request }) => {
        expect(request.headers.get("x-test-header")).toBe("kept");
        return HttpResponse.json({ space_uid: "demo", name: "Demo" });
      }),
      http.post(testApiUrl("/spaces/demo/entries"), async ({ request }) => {
        expect(request.headers.get("content-type")).toBe("application/json");
        expect(await request.json()).toEqual({ id: "entry-1" });
        return HttpResponse.json({ id: "entry-1", revision_id: "rev-1" });
      }),
    );

    await expect(
      protocolFetch(
        "space.get",
        { space_id: "demo" },
        undefined,
        { headers: { "x-test-header": "kept" } },
      ),
    ).resolves.toEqual({ space_uid: "demo", name: "Demo" });

    await expect(
      protocolFetch(
        "entry.create",
        { space_id: "demo" },
        { id: "entry-1" },
      ),
    ).resolves.toEqual({ id: "entry-1", revision_id: "rev-1" });
  });

  it("decodes the Composition save snapshot id as a JSON number", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/demo/compositions"),
        async ({ request }) => {
          expect(request.headers.get("idempotency-key")).toBe("attempt-1");
          expect(await request.json()).toEqual({ yaml: "canonical yaml" });
          return HttpResponse.json({
            composition_id: "tool-1",
            revision_id: "revision-2",
            canonical_yaml: "canonical yaml",
            receipt: {
              command_id: "command-1",
              catalog_generation: 4,
              snapshot_id: 42,
              committed_revision_ids: ["revision-2"],
              committed_at_micros: 1,
              data_file_count: 1,
            },
          });
        },
      ),
    );

    const response = await protocolFetch<CompositionSaveResponse>(
      "composition.save",
      { space_id: "demo", idempotency_key: "attempt-1" },
      { yaml: "canonical yaml" },
    );

    expect(typeof response.receipt.snapshot_id).toBe("number");
    expect(response.receipt.snapshot_id).toBe(42);
  });

  it("sends signed 64-bit Composition save inputs as exact JSON integers", async () => {
    server.use(
      http.post(
        testApiUrl("/spaces/demo/compositions"),
        async ({ request }) => {
          const body = await request.text();
          expect(body).toContain('"max":9223372036854775807');
          expect(body).toContain('"min":-9223372036854775808');
          return HttpResponse.json({
            composition_id: "tool-1",
            revision_id: "revision-2",
            canonical_yaml: "canonical yaml",
            receipt: {
              command_id: "command-1",
              catalog_generation: 4,
              snapshot_id: 42,
              committed_revision_ids: ["revision-2"],
              committed_at_micros: 1,
              data_file_count: 1,
            },
          });
        },
      ),
    );

    await compositionApi.save("demo", "canonical yaml", "attempt-1", {
      variables: {
        max: "9223372036854775807",
        min: "-9223372036854775808",
      },
      integerVariableIds: ["max", "min"],
    });
  });

  it("short-circuits fetch when a signal is aborted during WASM preparation", async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(
      protocolFetch(
        "space.get",
        { space_id: "demo" },
        undefined,
        { signal: controller.signal },
      ),
    ).rejects.toMatchObject({ name: "AbortError" });
  });

  it("uses an AbortError when an aborted signal has no reason", async () => {
    const signal = { aborted: true, reason: undefined } as AbortSignal;
    await expect(
      protocolFetch(
        "space.get",
        { space_id: "demo" },
        undefined,
        { signal },
      ),
    ).rejects.toMatchObject({ name: "AbortError" });
  });

  it("passes cancellation through to an in-flight browser fetch", async () => {
    let observedSignal: AbortSignal | undefined;
    server.use(
      http.get(testApiUrl("/spaces/demo"), async ({ request }) => {
        observedSignal = request.signal;
        await new Promise((resolve) => setTimeout(resolve, 100));
        return HttpResponse.json({ space_uid: "demo", name: "Demo" });
      }),
    );
    const controller = new AbortController();
    const request = protocolFetch(
      "space.get",
      { space_id: "demo" },
      undefined,
      { signal: controller.signal },
    );
    for (let attempt = 0; !observedSignal && attempt < 20; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    expect(observedSignal).toBeDefined();
    controller.abort();
    await expect(request).rejects.toMatchObject({ name: "AbortError" });
    expect(observedSignal?.aborted).toBe(true);
  });

  it("REQ-API-001: preserves multipart bodies and returns binary responses", async () => {
    const form = new FormData();
    form.append("file", new Blob(["contents"]), "note.txt");
    server.use(
      http.post(testApiUrl("/spaces/demo/assets"), ({ request }) => {
        expect(request.headers.get("content-type")).toContain(
          "multipart/form-data",
        );
        return HttpResponse.json({ status: "uploaded" });
      }),
      http.get(
        testApiUrl("/spaces/demo/assets/asset-1?form=Note&entry_id=entry-1"),
        () => new HttpResponse("asset bytes", { status: 200 }),
      ),
    );

    await expect(
      protocolFetch(
        "asset.upload",
        { space_id: "demo" },
        undefined,
        { body: form },
      ),
    ).resolves.toEqual({ status: "uploaded" });

    const response = await protocolFetchResponse("asset.read", {
      space_id: "demo",
      asset_id: "asset-1",
      form: "Note",
      entry_id: "entry-1",
    });
    await expect(response.text()).resolves.toBe("asset bytes");
  });

  it("REQ-API-001: exposes protocol errors with stable error-code fallbacks", () => {
    expect(
      new UgoiteApiError({
        kind: "invalid_arguments",
        message: "invalid",
        detail: { code: "from-detail" },
      }).code,
    ).toBe("from-detail");
    expect(
      new UgoiteApiError({
        kind: "invalid_arguments",
        message: "invalid",
        detail: { code: 42 },
        payload: { code: "from-payload" },
      }).code,
    ).toBe("from-payload");
    expect(
      new UgoiteApiError({
        kind: "invalid_arguments",
        message: "invalid",
        detail: null,
        payload: "not-an-object",
      }).code,
    ).toBeUndefined();
  });

  it("REQ-API-001: turns non-success protocol responses into UgoiteApiError", async () => {
    server.use(
      http.get(testApiUrl("/spaces/missing"), () =>
        HttpResponse.json({
          kind: "not_found",
          message: "space not found",
          code: "space_missing",
        }, { status: 404 })),
    );

    await expect(
      protocolFetch("space.get", { space_id: "missing" }),
    ).rejects.toMatchObject({
      name: "UgoiteApiError",
      code: "space_missing",
      status: 404,
    });

    server.use(
      http.get(
        testApiUrl("/spaces/missing/assets/asset-1"),
        () =>
          HttpResponse.json({
            kind: "not_found",
            message: "asset not found",
          }, { status: 404 }),
      ),
    );
    await expect(
      protocolFetchResponse("asset.read", {
        space_id: "missing",
        asset_id: "asset-1",
        form: "Note",
        entry_id: "entry-1",
      }),
    ).rejects.toMatchObject({ name: "UgoiteApiError", status: 404 });
  });

  it.each([
    [new TypeError("connection reset"), "connection reset", "TypeError"],
    ["response stream failed", "response stream failed", "unknown"],
  ])(
    "classifies a failed response-body read as an unknown write outcome (%s)",
    async (readFailure, message, cause) => {
      server.use(
        http.post(
          testApiUrl("/spaces/demo/entries"),
          () => HttpResponse.json({ id: "entry-1", revision_id: "rev-1" }),
        ),
      );
      vi.spyOn(Response.prototype, "text").mockRejectedValueOnce(readFailure);

      await expect(
        protocolFetch("entry.create", { space_id: "demo" }, { id: "entry-1" }),
      ).rejects.toMatchObject({
        name: "UgoiteApiError",
        kind: "transport",
        operation: "entry.create",
        status: 200,
        message,
        detail: { cause },
        mutationOutcome: "unknown",
        mutationCode: "MUTATION_OUTCOME_UNKNOWN",
      });
    },
  );

  it.each([
    [new TypeError("network offline"), "network offline", "TypeError"],
    ["socket closed", "socket closed", "unknown"],
  ])(
    "classifies a failed write fetch as an unknown outcome (%s)",
    async (fetchFailure, message, cause) => {
      vi.spyOn(globalThis, "fetch").mockRejectedValueOnce(fetchFailure);

      await expect(
        protocolFetch("entry.create", { space_id: "demo" }, { id: "entry-1" }),
      ).rejects.toMatchObject({
        name: "UgoiteApiError",
        kind: "transport",
        operation: "entry.create",
        message,
        detail: { cause },
        mutationOutcome: "unknown",
        mutationCode: "MUTATION_OUTCOME_UNKNOWN",
      });
    },
  );
});
