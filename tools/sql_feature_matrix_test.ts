import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url).pathname;
const read = (path: string) => Deno.readTextFile(`${root}${path}`);

const matrix = JSON.parse(await read("shared/sql/ugoite-sql-rules.json"))
  .feature_matrix;

Deno.test("SQL feature matrix binds the read-only gate and current UI actions", async () => {
  assertEquals(matrix.version, 1);
  assertEquals(
    matrix.backend.statement_gate,
    "exactly_one_select_query_statement",
  );
  assertEquals(matrix.backend.query_body_form_restricted, true);
  assertEquals(
    matrix.backend.clause_admission,
    "datafusion_parser_and_planner",
  );
  assertEquals(matrix.backend.closed_clause_allowlist, false);

  const [
    backendSource,
    toolbarSource,
    frontendSqlSource,
    sharedRules,
  ] = await Promise.all([
    read("crates/ugoite-iceberg/src/index.rs"),
    read("frontend/src/components/SqlStructureToolbar.tsx"),
    read("frontend/src/lib/sql.ts"),
    read("shared/sql/ugoite-sql-rules.json").then((contents) =>
      JSON.parse(contents)
    ),
  ]);
  const statementGate = backendSource.slice(
    backendSource.indexOf("pub fn validate_read_only_sql"),
    backendSource.indexOf(
      "pub async fn query_index_authorized",
      backendSource.indexOf("pub fn validate_read_only_sql"),
    ),
  );
  assertEquals(statementGate.includes("parsed.len() != 1"), true);
  assertEquals(
    statementGate.includes("query_contains_only_select_bodies(query)"),
    true,
  );

  assertEquals(
    matrix.direct_sql_lint.require_from,
    sharedRules.lint.require_from,
  );
  assertEquals(matrix.direct_sql_lint.require_from, false);
  assertEquals(matrix.direct_sql_lint.fromless_select_diagnostic, "none");
  const sqlLint = frontendSqlSource.slice(
    frontendSqlSource.indexOf("export function sqlLintDiagnostics"),
  );
  assertEquals(sqlLint.includes("lintRules.require_from"), false);
  assertEquals(
    sqlLint.includes('message: "Read-only query must include FROM"'),
    false,
  );

  const toolbarActions = toolbarSource.match(
    /type StructureAction\s*=([\s\S]*?);/,
  )?.[1]?.match(/"([a-z_]+)"/g)?.map((action) => action.slice(1, -1)) ?? [];
  const matrixActions = matrix.editor.operations.map((
    operation: { action: string },
  ) => operation.action);
  assertEquals([...matrixActions].sort(), [...toolbarActions].sort());

  const constructs = matrix.editor.constructs as Array<{
    id: string;
    backend_admission: string;
    editor_coverage: string;
    limits: string[];
  }>;
  assertEquals(
    new Set(constructs.map((construct) => construct.id)).size,
    constructs.length,
  );
  assertEquals(
    constructs.map((construct) => construct.id).sort(),
    [
      "common_table_expressions",
      "from_relation",
      "general_expressions",
      "grouping",
      "having",
      "joins",
      "ordering",
      "projection",
      "row_limit",
      "set_operations",
      "table_query",
      "where_predicate",
      "values_query",
    ].sort(),
  );
  assertEquals(
    constructs.find((construct) => construct.id === "projection")?.limits
      .includes(
        "relation-free integer/string/boolean/NULL literal projections",
      ),
    true,
  );
  const valuesQuery = constructs.find((construct) =>
    construct.id === "values_query"
  );
  assertEquals(
    valuesQuery?.backend_admission,
    "statement_gate_rejects",
  );
  assertEquals(
    constructs.every((construct) =>
      [
        "planner_decides",
        "statement_gate_passes_but_authorized_physical_plan_rejects",
        "statement_gate_rejects",
      ].includes(
        construct.backend_admission,
      )
    ),
    true,
  );
  assertEquals(
    constructs.every((construct) =>
      ["bounded", "context_only", "preserved_only", "none"].includes(
        construct.editor_coverage,
      )
    ),
    true,
  );
});

Deno.test("SQL feature matrix aggregate catalog remains present in the typed UI picker", async () => {
  const [frontendSql, domainSql] = await Promise.all([
    read("frontend/src/lib/sql.ts"),
    read("crates/ugoite-domain/src/sql_structure.rs"),
  ]);
  const aggregateCapability = frontendSql.slice(
    frontendSql.indexOf("const aggregateFunctionsForType"),
    frontendSql.indexOf("const sqlStructureTypeSupportsGrouping"),
  );
  const backendFunctions = matrix.backend.functions.aggregate as string[];
  const backendAggregateAliases = matrix.backend.function_aliases
    .aggregate as Record<string, string>;
  const editorFunctions = matrix.editor.operations.find(
    (operation: { action: string }) => operation.action === "aggregate",
  )?.functions as string[] | undefined;

  assertEquals(editorFunctions, backendFunctions);
  assertEquals(backendAggregateAliases, { mean: "avg" });
  assertEquals(
    matrix.editor.aggregate_contract.checked,
    "name_catalog_only",
  );
  assertEquals(
    matrix.editor.aggregate_contract.known_editor_rejections,
    ["COUNT(DISTINCT *)", "MEAN"],
  );
  assertEquals(
    matrix.editor.aggregate_contract.verification_gaps,
    [
      "runtime_structure_frontend_signature_and_type_parity",
      "supported_aggregate_modifier_parity",
      "count_distinct_wildcard_execution_semantics",
      "aggregate_alias_parity",
    ],
  );
  const aggregateParser = domainSql.slice(
    domainSql.indexOf("fn parse_aggregate("),
    domainSql.indexOf(
      "fn parse_group_by(",
      domainSql.indexOf("fn parse_aggregate("),
    ),
  );
  assertEquals(aggregateParser.includes("FunctionArgExpr::Wildcard"), true);
  assertEquals(aggregateParser.includes("if distinct"), true);
  assertEquals(
    aggregateParser.includes("SqlStructureReason::AggregateUnsupported"),
    true,
  );
  assertEquals(
    backendFunctions.every((name) =>
      new RegExp(`\\"${name}\\"`).test(aggregateCapability)
    ),
    true,
  );
});
