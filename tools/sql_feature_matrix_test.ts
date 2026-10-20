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
  assertEquals(
    constructs.find((construct) => construct.id === "projection")?.limits
      .includes(
        "simple Form-column projection aliases are inspected and edited independently by SQL-document-scoped projection slot identity",
      ),
    true,
  );
  assertEquals(
    constructs.find((construct) => construct.id === "projection")?.limits
      .includes(
        "repeated source columns remain editable, reorderable, and independently aliasable by projection slot",
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

Deno.test("SQL projection alias editing stays in the Rust-backed Columns contract", async () => {
  const [domainSql, toolbarSource, protocolSource] = await Promise.all([
    read("crates/ugoite-domain/src/sql_structure.rs"),
    read("frontend/src/components/SqlStructureToolbar.tsx"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
  ]);
  const projectionParser = domainSql.slice(
    domainSql.indexOf("fn parse_projection("),
    domainSql.indexOf(
      "fn parse_aggregate(",
      domainSql.indexOf("fn parse_projection("),
    ),
  );
  assertEquals(
    projectionParser.includes("expr: Expr::Identifier(identifier)"),
    true,
  );
  assertEquals(domainSql.includes("fn rewrite_projection_aliases("), true);
  assertEquals(toolbarSource.includes("projection_aliases"), true);
  assertEquals(
    toolbarSource.includes('"sqlPage.structure.aliasForSlot"'),
    true,
  );
  assertEquals(
    protocolSource.includes("aliases?: (SqlStructureAlias | null)[]"),
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
  assertEquals(matrix.editor.literal_projection_operation, {
    action: "columns",
    protocol: "set_literal_projection",
    sql: ["relation-free SELECT literal projection"],
    coverage: "bounded",
    values: ["integer", "string", "boolean", "null"],
  });
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

Deno.test("JOIN editor coverage is bounded without changing runtime admission", async () => {
  const [backendSource, domainSource, toolbarSource] = await Promise.all([
    read("crates/ugoite-iceberg/src/index.rs"),
    read("crates/ugoite-domain/src/sql_structure.rs"),
    read("frontend/src/components/SqlStructureToolbar.tsx"),
  ]);
  const joinConstruct = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "joins");
  assertEquals(joinConstruct.backend_admission, "planner_decides");
  assertEquals(joinConstruct.editor_coverage, "bounded");
  assertEquals(
    joinConstruct.limits.includes(
      "one INNER self-join of the selected Form with explicit distinct aliases",
    ),
    true,
  );
  assertEquals(
    joinConstruct.limits.includes(
      "ON must be left_alias.column = right_alias.column with matching supported scalar types; reversed equality operands fail closed",
    ),
    true,
  );
  assertEquals(
    joinConstruct.limits.includes(
      "WHERE supports bounded AND/OR/NOT trees of qualified parameter comparisons plus IS NULL/IS NOT NULL on qualified columns; adding a JOIN qualifies every supported predicate leaf; ORDER BY supports simple qualified columns with ASC/DESC and no NULLS FIRST/LAST; unqualified references fail closed",
    ),
    true,
  );
  assertEquals(
    joinConstruct.limits.includes(
      "adding a join qualifies existing supported single-relation WHERE/ORDER BY columns with the left alias and preserves surrounding clause text",
    ),
    true,
  );
  assertEquals(
    matrix.editor.operations.some((operation: { action: string }) =>
      operation.action === "join"
    ),
    true,
  );
  assertEquals(
    matrix.editor.operations.find((operation: { action: string }) =>
      operation.action === "join"
    )?.toolbar_category,
    "structure",
  );
  assertEquals(
    toolbarSource.includes('kind: action === "join" ? "structure" : action'),
    true,
  );
  assertEquals(toolbarSource.includes('? "plus"'), false);
  assertEquals(toolbarSource.includes('"join"'), true);
  assertEquals(domainSource.includes("JoinOperator::Inner"), true);
  assertEquals(domainSource.includes("resolve_column_reference"), true);
  assertEquals(domainSource.includes("qualify_join_input_references"), true);
  assertEquals(
    domainSource.includes("SqlStructureReason::JoinUnsupported"),
    true,
  );

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
});

Deno.test("ORDER BY editor resolves projection aliases before Form columns", () => {
  const ordering = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "ordering");
  assertEquals(ordering.backend_admission, "planner_decides");
  assertEquals(ordering.editor_coverage, "bounded");
  assertEquals(
    ordering.limits.includes(
      "unqualified identifiers resolve explicit projection aliases before same-named Form columns; the editor retains the typed source binding",
    ),
    true,
  );
});
