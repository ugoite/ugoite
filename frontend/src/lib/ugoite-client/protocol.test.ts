import { http, HttpResponse } from "msw";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  analyzeSqlStructure,
  applySqlDocumentEditTransaction,
  applySqlDocumentNodeEditTransaction,
  applySqlDocumentReplacementTransaction,
  buildSpreadsheetCsvRequest,
  canonicalizeCompositionDocument,
  compileVariableGraph,
  encodeSpreadsheetCsv,
  getWasmSupportedOperations,
  parseSqlDocument,
  parseVariableGraphInput,
  parseVariableGraphInputSync,
  prepareApiRequest,
  prepareSqlStructureParser,
  prepareVariableGraphInputParser,
  proposeSqlStructureOperation,
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
          value: { kind: "integer", value: 1 },
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

  it("analyzes SQL structure and returns confirmation proposals through Rust/WASM", async () => {
    await prepareSqlStructureParser();

    const sql =
      'SELECT * FROM "Task" WHERE "field_101" = $status ORDER BY "_ugoite_id"';
    const context = {
      relation: "Task",
      columns: [
        { name: "_ugoite_id", type: "string" },
        { name: "field_101", type: "string" },
        { name: "field_102", type: "integer" },
      ],
      variables: [
        { name: "status", type: "string" },
        { name: "priority", type: "integer" },
      ],
      stable_key: ["_ugoite_id"],
    } as const;

    const analysis = await analyzeSqlStructure(sql, context);
    expect(analysis).toMatchObject({
      status: "supported",
      projection: ["*"],
      referenced_variables: ["status"],
      stable_pagination: true,
    });

    const proposal = await proposeSqlStructureOperation(
      sql,
      context,
      { kind: "set_columns", columns: ["field_101", "field_102"] },
    );
    expect(proposal).toMatchObject({
      status: "supported",
      base_sql: sql,
      requires_confirmation: true,
    });
    expect(proposal.proposed_sql).toContain('"field_101", "field_102"');
    expect(sql).toContain('SELECT * FROM "Task"');

    const repeatedProjectionSql =
      'SELECT "field_101" AS first, "field_101" AS second FROM "Task"';
    const repeatedProjection = await analyzeSqlStructure(
      repeatedProjectionSql,
      context,
    );
    expect(repeatedProjection.projection_aliases).toEqual([
      { column: "field_101", alias: { name: "first", quoted: false } },
      { column: "field_101", alias: { name: "second", quoted: false } },
    ]);
    const slotProposal = await proposeSqlStructureOperation(
      repeatedProjectionSql,
      context,
      {
        kind: "set_projection_slots",
        slots: [
          {
            id: "projection-1",
            column: "field_101",
            alias: { name: "second_copy", quoted: false },
          },
          {
            id: "projection-0",
            column: "field_101",
            alias: { name: "first_copy", quoted: false },
          },
        ],
      },
    );
    expect(slotProposal).toMatchObject({
      status: "supported",
      base_sql: repeatedProjectionSql,
      requires_confirmation: true,
    });
    expect(slotProposal.proposed_sql).toContain(
      '"field_101" AS second_copy, "field_101" AS first_copy',
    );

    const aggregateSql =
      'SELECT DISTINCT "field_101", COUNT(DISTINCT "field_102") AS unique_count FROM "Task" GROUP BY "field_101"';
    const aggregateAnalysis = await analyzeSqlStructure(aggregateSql, context);
    expect(aggregateAnalysis).toMatchObject({
      status: "supported",
      projection: ["field_101"],
      aggregates: [{
        function: "count",
        column: "field_102",
        distinct: true,
        alias: { name: "unique_count", quoted: false },
      }],
      group_by: ["field_101"],
      distinct: true,
    });

    const aggregateProposal = await proposeSqlStructureOperation(
      'SELECT "field_101" FROM "Task" GROUP BY "field_101"',
      context,
      {
        kind: "set_aggregates",
        aggregates: [{
          function: "sum",
          column: "field_102",
          distinct: true,
          alias: { name: "total", quoted: true },
        }],
      },
    );
    expect(aggregateProposal).toMatchObject({
      status: "supported",
      requires_confirmation: true,
    });
    expect(aggregateProposal.proposed_sql).toContain(
      'SUM(DISTINCT "field_102") AS "total"',
    );

    const distinctProposal = await proposeSqlStructureOperation(
      'SELECT "field_101" FROM "Task"',
      context,
      { kind: "set_distinct", distinct: true },
    );
    expect(distinctProposal.proposed_sql).toContain("SELECT DISTINCT");
  });

  it("parses bounded SQL drafts through the typed Rust/WASM document contract", async () => {
    const sql = '-- 日本語\nSELECT $day FROM "G"';
    const document = await parseSqlDocument(sql);
    const sourceBytes = new TextEncoder().encode(sql);

    expect(document).toMatchObject({
      contract_version: 1,
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

    await expect(parseSqlDocument("SELECT (")).resolves.toMatchObject({
      source: "SELECT (",
      status: "invalid_sql",
      diagnostics: [{ code: "parse_failed" }],
    });
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

  it("applies proposal replacements through a source-preserving Rust transaction", async () => {
    const source = 'SELECT a /* keep */, b FROM "G" WHERE id = 1 -- tail';
    const replacement =
      'SELECT total_a /* keep */, total_b FROM "G" WHERE id = 2 -- tail';

    const edited = await applySqlDocumentReplacementTransaction(
      source,
      replacement,
    );
    expect(edited.document.source).toBe(replacement);
    expect(edited.document.status).toBe("parsed");
    expect(edited.undo.edits.length).toBeGreaterThan(1);
    expect(
      edited.undo.edits.every((edit) =>
        !edit.expected_text.includes("keep") &&
        !edit.expected_text.includes("tail") &&
        !edit.replacement.includes("keep") &&
        !edit.replacement.includes("tail")
      ),
    ).toBe(true);

    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("applies source-guarded SQL Document node edits through Rust", async () => {
    const source = 'SELECT * FROM "G" ORDER BY value -- keep trailing comment';
    const document = await parseSqlDocument(source);
    const item = document.nodes.find((node) => node.kind === "select_item");
    expect(item).toBeDefined();

    const edited = await applySqlDocumentNodeEditTransaction(source, {
      contract_version: document.contract_version,
      expected_source: source,
      edits: [{
        node_id: item!.id,
        expected_kind: item!.kind,
        replacement: "value AS selected",
      }],
    });
    expect(edited.document.source).toBe(
      'SELECT value AS selected FROM "G" ORDER BY value -- keep trailing comment',
    );
    expect(edited.document.status).toBe("parsed");

    const restored = await applySqlDocumentEditTransaction(
      edited.document.source,
      edited.undo,
    );
    expect(restored.document.source).toBe(source);
  });

  it("exposes source-ranged query clauses and relationships from the Rust document", async () => {
    const document = await parseSqlDocument(
      'WITH source_rows AS (SELECT id FROM "G") SELECT DISTINCT id FROM source_rows LEFT JOIN "H" AS h ON source_rows.id = h.id WHERE h.id > 0 GROUP BY id HAVING COUNT(*) > 0 ORDER BY id DESC LIMIT 10 OFFSET 2',
    );
    expect(document.status).toBe("parsed");
    const kinds = new Set(document.nodes.map((node) => node.kind));
    for (
      const kind of [
        "with_clause",
        "common_table_expression",
        "distinct_clause",
        "from_clause",
        "join",
        "where_clause",
        "group_by_clause",
        "having_clause",
        "order_by_clause",
        "row_limit_clause",
      ] as const
    ) {
      expect(kinds.has(kind)).toBe(true);
    }

    const parentOf = (kind: typeof document.nodes[number]["kind"]) =>
      document.nodes.find((node) => node.kind === kind);
    const withClause = parentOf("with_clause");
    const cte = parentOf("common_table_expression");
    expect(cte?.parent).toBe(withClause?.id);
    const join = parentOf("join");
    expect(document.nodes.find((node) => node.id === join?.parent)?.kind)
      .toBe("from_clause");
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
  });

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
        value: 9_007_199_254_740_991,
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
      value: 42,
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
