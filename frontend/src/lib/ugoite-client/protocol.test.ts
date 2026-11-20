import { http, HttpResponse } from "msw";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applySqlDocumentSemanticEditTransaction,
  buildSpreadsheetCsvRequest,
  canonicalizeCompositionDocument,
  collectSqlFormNameReferences,
  compileVariableGraph,
  compositionFormQuerySql,
  encodeSpreadsheetCsv,
  getWasmSupportedOperations,
  parseCompositionTextTemplate,
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

describe("portable Ugoite API protocol WASM", () => {
  afterEach(() => vi.restoreAllMocks());

  it("parses Text templates and escaped literal markers in Rust", async () => {
    await expect(
      parseCompositionTextTemplate("A {{total}} ! \\{{literal}}"),
    ).resolves.toEqual({
      segments: [
        { kind: "literal", text: "A " },
        { kind: "variable", variable: "total" },
        { kind: "literal", text: " ! {{literal}}" },
      ],
    });
  });

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

  it("constructs explicit Form projections and metric SQL through Rust", async () => {
    expect(
      await compositionFormQuerySql('form"identity', [{
        column: "title",
        label: "Title",
      }]),
    ).toEqual({
      sql:
        'SELECT "title" AS "Title" FROM "form""identity" ORDER BY "_ugoite_updated_at" DESC, "_ugoite_id"',
      projection_aliases: ["Title"],
    });
    expect(
      await compositionFormQuerySql("form_id", [], { kind: "count" }),
    ).toEqual({
      sql: 'SELECT COUNT(*) AS value FROM "form_id"',
      projection_aliases: ["value"],
    });
    expect(
      await compositionFormQuerySql(
        "form_id",
        [],
        { kind: "sum", column: 'a"b' },
      ),
    ).toEqual({
      sql: 'SELECT SUM("a""b") AS value FROM "form_id"',
      projection_aliases: ["value"],
    });
    await expect(compositionFormQuerySql("", [])).rejects.toThrow();
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

  it("returns the Rust diagnostic for invalid Composition documents", async () => {
    await expect(
      canonicalizeCompositionDocument(
        { ...typedCompositionDocument, unknown: true },
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
      contract_version: 27,
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

  it("initializes an empty SQL draft through the Form picker", async () => {
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
    await expect(applySqlDocumentSemanticEditTransaction("SELECT (", {
      contract_version: parsed.contract_version,
      expected_source: "SELECT (",
      edits: [{ kind: "initialize_select", relation: null }],
    })).rejects.toMatchObject({
      kind: "sql_document_edit",
      code: "document_not_parsed",
    });
  });

  it("exposes SQL Document edit codes through the typed API error contract", () => {
    const sqlEditError = new UgoiteApiError<SqlDocumentEditErrorCode>({
      kind: "sql_document_edit",
      message: "The SQL edit node has no replaceable source range",
      code: "node_range_unavailable",
    });
    const sqlEditCode: SqlDocumentEditErrorCode | undefined = sqlEditError.code;
    expect(sqlEditCode).toBe("node_range_unavailable");

    const unrelatedError = new UgoiteApiError({
      kind: "not_found",
      message: "Space was not found",
      code: "space_missing",
    });
    const unrelatedApiCode: string | undefined = unrelatedError.code;
    expect(unrelatedApiCode).toBe("space_missing");
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
      '{"evaluation_id":"eval-1","ok":true,"computed_query_ids":[],"variable_values":{"max":{"kind":"integer","value":9223372036854775807},"min":{"kind":"integer","value":-9223372036854775808}},"queries":[],"component_bindings":[],"dependencies":{},"query_results":{"scalar":{"value":{"kind":"scalar","value":{"kind":"integer","value":9223372036854775807}},"has_more":false},"rows":{"value":{"kind":"table","columns":[{"name":"max","value_type":{"scalar_type":"integer","nullable":false}},{"name":"json","value_type":{"scalar_type":"json","nullable":false}}],"rows":[[{"kind":"integer","value":9223372036854775807},{"kind":"json","value":{"kind":"integer","value":7}}]]},"has_more":false}}}';
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
    expect(response.query_results.scalar.value).toEqual({
      kind: "scalar",
      value: { kind: "integer", value: "9223372036854775807" },
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

  it("preserves unsafe entry integers through get and unrelated update serialization", async () => {
    const responseBody =
      '{"id":"entry-1","form":"form-1","fields":{"count":9007199254740993,"counts":[9007199254740995,7],"objects":[{"count":9007199254740997}],"decimal":1.25,"exponent":1e20},"extra_attributes":{"count":9007199254740999}}';
    server.use(
      http.get(
        testApiUrl("/spaces/demo/entries/entry-1"),
        () =>
          new Response(responseBody, {
            headers: { "content-type": "application/json" },
          }),
      ),
    );
    const entry = await protocolFetch<{
      fields: Record<string, unknown>;
      extra_attributes: Record<string, unknown>;
    }>("entry.get", { space_id: "demo", entry_id: "entry-1" });
    expect(entry.fields.count).toBe("9007199254740993");
    expect(entry.fields.counts).toEqual(["9007199254740995", 7]);
    expect(entry.fields.objects).toEqual([{ count: 9007199254740997 }]);
    expect(entry.fields.decimal).toBe(1.25);
    expect(entry.fields.exponent).toBe(1e20);
    expect(entry.extra_attributes.count).toBe(9007199254740999);

    const update = await prepareApiRequest("entry.update", {
      space_id: "demo",
      entry_id: "entry-1",
    }, { fields: entry.fields });
    expect(JSON.parse(update.body ?? "null").fields).toEqual({
      count: "9007199254740993",
      counts: ["9007199254740995", 7],
      objects: [{ count: 9007199254740997 }],
      decimal: 1.25,
      exponent: 1e20,
    });

    server.use(
      http.get(
        testApiUrl("/spaces/demo/entries/entry-empty"),
        () =>
          new Response('{"id":"entry-empty"}', {
            headers: { "content-type": "application/json" },
          }),
      ),
    );
    await expect(
      protocolFetch("entry.get", {
        space_id: "demo",
        entry_id: "entry-empty",
      }),
    ).resolves.toEqual({ id: "entry-empty" });
  });

  it("prepares the portable offline SQL lint operation", async () => {
    const request = await prepareApiRequest(
      "sql.lint",
      {},
      { sql: 'SELECT * FROM "Expense"', profile: "composition" },
    );

    expect(request).toMatchObject({
      method: "POST",
      path: "/sql/lint",
      body_kind: "json",
    });
    expect(JSON.parse(request.body ?? "null")).toEqual({
      sql: 'SELECT * FROM "Expense"',
      profile: "composition",
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
