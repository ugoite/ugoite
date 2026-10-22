import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url).pathname;
const read = (path: string) => Deno.readTextFile(`${root}${path}`);

const matrix = JSON.parse(await read("shared/sql/ugoite-sql-rules.json"))
  .feature_matrix;

Deno.test("SQL feature matrix binds the read-only gate and current UI actions", async () => {
  assertEquals(matrix.version, 12);
  assertEquals(
    matrix.backend.statement_gate,
    "exactly_one_select_query_statement",
  );
  assertEquals(matrix.backend.query_body_form_restricted, true);
  assertEquals(
    matrix.backend.clause_admission,
    "datafusion_parser_and_planner",
  );
  assertEquals(matrix.backend.parser_engine, {
    crate: "datafusion",
    version: "54.1.0",
    recursion_limit: 256,
  });
  assertEquals(matrix.backend.parser_dialect, "GenericDialect");
  assertEquals(matrix.editor.parser_dialect, matrix.backend.parser_dialect);
  assertEquals(matrix.backend.closed_clause_allowlist, false);

  const [
    backendSource,
    toolbarSource,
    frontendSqlSource,
    sqlDocumentSource,
    sqlStructureSource,
    clientProtocol,
    sharedRules,
    cargoLock,
    icebergManifest,
    backendParserSource,
    queryContextSource,
  ] = await Promise.all([
    read("crates/ugoite-iceberg/src/index.rs"),
    read("frontend/src/components/SqlStructureToolbar.tsx"),
    read("frontend/src/lib/sql.ts"),
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-domain/src/sql_structure.rs"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
    read("shared/sql/ugoite-sql-rules.json").then((contents) =>
      JSON.parse(contents)
    ),
    read("Cargo.lock"),
    read("crates/ugoite-iceberg/Cargo.toml"),
    read("crates/ugoite-iceberg/src/sql_parser.rs"),
    read("crates/ugoite-iceberg/src/query_context.rs"),
  ]);
  assertEquals(
    backendParserSource.includes("SQL_PARSER_RECURSION_LIMIT: usize = 256"),
    true,
  );
  assertEquals(
    sqlDocumentSource.includes(
      "SQL_DOCUMENT_PARSER_RECURSION_LIMIT: usize = 256",
    ),
    true,
  );
  assertEquals(backendParserSource.includes("with_recursion_limit("), true);
  assertEquals(
    queryContextSource.includes('"datafusion.sql_parser.recursion_limit"'),
    true,
  );
  assertEquals(
    sqlDocumentSource.includes("use sqlparser::dialect::GenericDialect;"),
    true,
  );
  assertEquals(
    sqlStructureSource.includes("parse_generic_sql_statements"),
    true,
  );
  assertEquals(sqlStructureSource.includes("PostgreSqlDialect"), false);
  const datafusionLockPackage = cargoLock.match(
    /^name = "datafusion"\nversion = "([^"]+)"/m,
  )?.[1];
  assertEquals(datafusionLockPackage, matrix.backend.parser_engine.version);
  const datafusionDependency = icebergManifest.match(
    /^datafusion = "([^"]+)"/m,
  )?.[1];
  assertEquals(datafusionDependency, matrix.backend.parser_engine.version);
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
  const setExprValidatorStart = backendSource.indexOf(
    "fn set_expr_is_select_only(",
  );
  const setExprValidatorEnd = backendSource.indexOf(
    "struct SelectBodyValidator",
    setExprValidatorStart,
  );
  const setExprValidator = backendSource.slice(
    setExprValidatorStart,
    setExprValidatorEnd,
  );
  const actualQueryBodyKinds = [...setExprValidator.matchAll(
    /SetExpr::([A-Za-z0-9_]+)\s*(?:\([^)]*\)|\{[^}]*\})?\s*=>/g,
  )].map(([, kind]) =>
    kind.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase()
  );
  assertEquals(
    [...actualQueryBodyKinds].sort(),
    [...matrix.backend.query_body_policy.allowed_set_expr_kinds].sort(),
  );
  assertEquals(setExprValidator.includes("_ => false"), true);
  assertEquals(
    matrix.backend.query_body_policy.nested_queries,
    "visit_every_query_node",
  );
  assertEquals(
    matrix.backend.query_body_policy.unknown_set_expr,
    "reject",
  );
  const nestedQueryValidatorStart = backendSource.indexOf(
    "fn query_contains_only_select_bodies(",
  );
  const nestedQueryValidatorEnd = backendSource.indexOf(
    "pub async fn query_index_authorized(",
    nestedQueryValidatorStart,
  );
  const nestedQueryValidator = backendSource.slice(
    nestedQueryValidatorStart,
    nestedQueryValidatorEnd,
  );
  assertEquals(
    nestedQueryValidator.includes("fn pre_visit_query"),
    true,
  );
  assertEquals(
    nestedQueryValidator.includes(
      "set_expr_is_select_only(query.body.as_ref())",
    ),
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
  assertEquals(
    matrix.editor.operations.find((operation: { action: string }) =>
      operation.action === "row_limit"
    )?.protocol,
    "set_row_limit",
  );
  assertEquals(
    clientProtocol.includes('kind: "set_row_limit"'),
    true,
  );
  assertEquals(
    clientProtocol.includes("row_limit: SqlStructureActionCapability"),
    true,
  );
  assertEquals(
    clientProtocol.includes("row_limit?: SqlStructureRowLimit"),
    true,
  );

  const constructs = matrix.editor.constructs as Array<{
    id: string;
    sql: string;
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
      typeof construct.sql === "string" && construct.sql.length > 0 &&
      Object.hasOwn(construct, "editor_coverage")
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
  const whereConstruct = constructs.find((construct) =>
    construct.id === "where_predicate"
  );
  assertEquals(
    whereConstruct?.limits.some((limit) => limit.includes("nested AND/OR/NOT")),
    true,
  );
  assertEquals(
    whereConstruct?.limits.some((limit) => limit.includes("no OR or NOT")),
    false,
  );
});

Deno.test("executor-admitted constructs map to the shared typed SQL document", async () => {
  const [sqlDocumentSource, protocolSource] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
  ]);
  const contract = matrix.editor.typed_document_contract as {
    version: number;
    protocol: string;
    parser_engine: { crate: string; version: string };
    parser_dialect: string;
    statement_nodes: string[];
    semantic_protocol: string;
    semantic_kinds: string[];
    node_kinds: string[];
  };
  const rustVersion = sqlDocumentSource.match(
    /pub const SQL_DOCUMENT_CONTRACT_VERSION:\s*u32\s*=\s*(\d+);/,
  )?.[1];
  assertEquals(contract.version, Number(rustVersion));
  assertEquals(contract.protocol, "SqlDocumentNodeKind");
  assertEquals(contract.semantic_protocol, "SqlDocumentNodeData");
  assertEquals(contract.parser_engine, matrix.backend.parser_engine);
  assertEquals(contract.parser_dialect, matrix.backend.parser_dialect);

  const rustNodeKindBody = sqlDocumentSource.match(
    /pub enum SqlDocumentNodeKind\s*\{([\s\S]*?)\n\}/,
  )?.[1];
  const rustNodeKinds = [...(rustNodeKindBody ?? "").matchAll(
    /^\s*([A-Z][A-Za-z0-9_]*)\s*,/gm,
  )].map(([, name]) =>
    name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase()
  );
  const rustSemanticBody = sqlDocumentSource.match(
    /pub enum SqlDocumentNodeData\s*\{([\s\S]*?)\n\}/,
  )?.[1];
  const rustSemanticKinds = [...(rustSemanticBody ?? "").matchAll(
    /^\s{4}([A-Z][A-Za-z0-9_]*)\s*\{/gm,
  )].map(([, name]) =>
    name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase()
  );
  const tsNodeKindBody = protocolSource.match(
    /export type SqlDocumentNodeKind\s*=([\s\S]*?);/,
  )?.[1];
  const tsNodeKinds = [...(tsNodeKindBody ?? "").matchAll(/"([a-z_]+)"/g)]
    .map(([, name]) => name);
  assertEquals([...contract.node_kinds].sort(), [...rustNodeKinds].sort());
  assertEquals([...contract.node_kinds].sort(), [...tsNodeKinds].sort());
  const semanticTypeStart = protocolSource.indexOf(
    "export type SqlDocumentNodeData =",
  );
  const semanticTypeEnd = protocolSource.indexOf(
    "\nexport type SqlDocumentNode =",
    semanticTypeStart,
  );
  const tsSemanticBody = protocolSource.slice(
    semanticTypeStart,
    semanticTypeEnd,
  );
  const tsSemanticKinds = [...(tsSemanticBody ?? "").matchAll(
    /\bkind:\s*"([a-z_]+)"/g,
  )].map(([, name]) => name);
  assertEquals(
    [...contract.semantic_kinds].sort(),
    [...rustSemanticKinds].sort(),
  );
  assertEquals(
    [...contract.semantic_kinds].sort(),
    [...tsSemanticKinds].sort(),
  );
  assertEquals(
    protocolSource.includes("semantic: SqlDocumentNodeData | null"),
    true,
  );

  const constructs = matrix.editor.constructs as Array<{
    id: string;
    backend_admission: string;
    document_handling: string;
    document_nodes: string[];
  }>;
  const constructIds = constructs.map((construct) => construct.id);
  assertEquals(new Set(constructIds).size, constructIds.length);
  for (const construct of constructs) {
    if (construct.backend_admission === "planner_decides") {
      assertEquals(construct.document_handling, "typed_tree");
      assertEquals(construct.document_nodes.length > 0, true);
    } else {
      assertEquals(construct.backend_admission, "statement_gate_rejects");
      assertEquals(construct.document_handling, "rejected_by_executor_gate");
      assertEquals(construct.document_nodes, []);
    }
    assertEquals(
      construct.document_nodes.every((kind) =>
        contract.node_kinds.includes(kind)
      ),
      true,
      `${construct.id} must use declared SQL Document node kinds`,
    );
  }
  const representedNodeKinds = new Set([
    ...contract.statement_nodes,
    ...constructs.flatMap((construct) => construct.document_nodes),
  ]);
  assertEquals(
    [...representedNodeKinds].sort(),
    [...contract.node_kinds].sort(),
  );
});

Deno.test("runtime and editor parsers share a checked grammar fixture corpus", async () => {
  const fixturePath = "shared/sql/sql-document-parser-parity.json";
  assertEquals(matrix.backend.parser_parity_fixture, fixturePath);
  assertEquals(matrix.editor.parser_parity_fixture, fixturePath);

  const fixture = JSON.parse(await read(fixturePath)) as {
    version: number;
    cases: Array<{ id: string; sql: string }>;
  };
  assertEquals(fixture.version, 1);
  if (fixture.cases.length < 10) {
    throw new Error("parser parity corpus must cover all SQL tree families");
  }
  const ids = fixture.cases.map((item) => item.id);
  assertEquals(new Set(ids).size, ids.length, "parser parity case IDs are unique");
  for (const item of fixture.cases) {
    if (!item.sql.startsWith("SELECT ") && !item.sql.startsWith("WITH ")) {
      throw new Error(`parser parity case ${item.id} is outside SELECT grammar`);
    }
  }

  const [domainSource, executorSource] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-iceberg/src/sql_parser.rs"),
  ]);
  assertEquals(
    domainSource.includes("fn shared_parser_parity_cases_are_parsed_by_the_sql_document()"),
    true,
  );
  assertEquals(
    executorSource.includes(
      "fn shared_parser_parity_cases_are_accepted_by_datafusion_and_sql_document()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(fixturePath) && executorSource.includes(fixturePath),
    true,
    "both native parser tests consume the same checked corpus",
  );
});

Deno.test("set-operation tree edits are typed and verified end to end", async () => {
  const [
    domainSource,
    wasmSource,
    frontendProtocolTest,
    editorTest,
    authorizedQueryTests,
  ] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  const setOperations = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "set_operations");
  assertEquals(setOperations.backend_admission, "planner_decides");
  assertEquals(setOperations.editor_coverage, "bounded");
  assertEquals(setOperations.editor_operations, ["document"]);
  assertEquals(setOperations.verification, [
    "domain_edit_undo",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    setOperations.limits.some((limit: string) =>
      limit.includes(
        "a typed swap operation reorders the exact operand source ranges",
      )
    ),
    true,
  );
  assertEquals(
    matrix.editor.operations.find((operation: { action: string }) =>
      operation.action === "document"
    )?.coverage,
    "typed identifier, literal, binary operator, arithmetic unary sign, CAST/TRY_CAST/SAFE_CAST/::/CONVERT/TRY_CONVERT target type and CAST wrapper, registered aggregate wrapper, searched CASE wrapper, typed IN/NOT IN/BETWEEN/NOT BETWEEN/LIKE/NOT LIKE/IS NULL/IS NOT NULL predicate templates, editable IN-list value append/remove, WHERE clause seed, CTE rename/add-seed/remove-unreferenced, set-operation operator/quantifier/add-seed/remove-one-operand/swap-operands, supported predicate negation, CASE branch/ELSE, typed projection expression replacement, binary expression construction on a selected SELECT item or expression node from same-query typed operands, same-query expression replacement, projection append/removal/reorder, GROUP BY expression append, and ORDER BY expression append; reorder is limited to comment-free SELECT lists and removal rejects comments in the removed span; aggregate argument edits follow the Rust registry, and scalar subquery projection seed",
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_set_operation_edit_preserves_comments_and_supports_undo()",
    ) &&
      domainSource.includes(
        "fn semantic_set_operation_add_seed_and_operand_removal_are_typed_and_undoable()",
      ) &&
      domainSource.includes(
        "fn semantic_set_operation_operands_can_be_swapped_and_undone()",
      ) &&
      domainSource.includes(
        "fn semantic_set_operation_operand_removal_fails_closed_when_comments_would_be_dropped()",
      ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_set_operation_semantic_edit_protocol_matches_native()",
    ) &&
      wasmSource.includes(
        "fn sql_document_set_operation_add_remove_protocol_matches_native()",
      ) &&
      wasmSource.includes(
        "fn sql_document_set_operation_swap_protocol_matches_native()",
      ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("edits set operators and quantifiers through the Rust protocol"',
    ) &&
      frontendProtocolTest.includes(
        'it("swaps set-operation operands through the Rust protocol"',
      ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("appends a set-operation seed from a query node through Rust"',
    ) &&
      editorTest.includes(
        'it("removes an explicitly selected set-operation operand through Rust"',
      ) &&
      editorTest.includes(
        'it("swaps set-operation operands through a guarded Rust edit"',
      ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "SqlDocumentSemanticEdit::AddSetOperationOperand",
    ) &&
      authorizedQueryTests.includes(
        "SqlDocumentSemanticEdit::RemoveSetOperationOperand",
      ) &&
      authorizedQueryTests.includes(
        "the appended typed set-operation seed reaches the authorized executor",
      ) &&
      authorizedQueryTests.includes(
        "fn typed_set_operation_operand_swap_reaches_authorized_execution()",
      ),
    true,
  );
});

Deno.test("CAST target editing is typed and reaches authorized execution", async () => {
  const [
    domainSource,
    wasmSource,
    frontendProtocolTest,
    editorTest,
    authorizedQueryTests,
  ] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  const expressions = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "general_expressions");
  assertEquals(expressions.editor_coverage, "bounded");
  assertEquals(expressions.editor_operations, ["document"]);
  assertEquals(expressions.verification, [
    "domain_edit_undo",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSource.includes(
      "fn semantic_cast_target_edit_preserves_operand_and_supports_undo()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_cast_target_edit_fails_closed_when_comments_hide_the_wrapper()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_cast_target_edit_preserves_every_supported_cast_form_and_undoes()",
    ) && domainSource.includes("fn cast_target_type_source_range(") &&
      editorTest.includes(
        "edits TRY_CAST target types without replacing the cast form",
      ) && wasmSource.includes("SELECT TRY_CAST(amount + 1 AS DOUBLE)"),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_unary_sign_edit_preserves_operand_comments_and_supports_undo()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_case_structure_edits_add_remove_branches_and_else_losslessly()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes("SetCaseElse {") &&
      domainSource.includes("fn change_case_branch("),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_function_argument_edits_copy_typed_nodes_and_support_undo()",
    ) && domainSource.includes(
      "fn semantic_function_arguments_follow_registry_arity_and_preserve_comments()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_projection_expression_edits_copy_typed_nodes_and_support_undo()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_projection_items_can_be_removed_moved_and_undone()",
    ) && domainSource.includes(
      "fn semantic_projection_item_edits_preserve_or_reject_ambiguous_comments()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes("AddProjectionExpression {") &&
      domainSource.includes("fn add_projection_expression("),
    true,
  );
  assertEquals(
    domainSource.includes("AddScalarSubqueryProjection {") &&
      domainSource.includes("fn add_scalar_subquery_projection(") &&
      domainSource.includes(
        "fn semantic_scalar_subquery_projection_is_typed_parseable_and_undoable()",
      ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("edits TRY_CAST target types without replacing the cast form"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("edits a literal inside a CASE aggregate expression through Rust"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("adds a CASE branch from the syntax tree controls"',
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_cast_target_semantic_edit_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_unary_sign_semantic_edit_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_case_structure_edit_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_function_argument_edit_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_projection_expression_edit_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_projection_expression_add_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_projection_item_edits_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("edits CAST target types through the Rust protocol"',
    ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("edits arithmetic unary signs through the Rust protocol and preserves comments"',
    ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("adds and removes CASE branches through the Rust protocol"',
    ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("edits aggregate arguments from typed expression nodes through Rust"',
    ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("replaces a SELECT expression with another typed tree expression"',
    ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("adds a SELECT expression from another typed node in Rust"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("adds a COUNT argument from another typed expression node"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("replaces a selected projection with another typed expression"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("adds another SELECT item from a typed expression in the query"',
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn semantic_binary_expression_uses_same_query_nodes_and_supports_undo()",
    ) && domainSource.includes(
      "fn semantic_group_by_expression_adds_before_later_clauses_and_supports_undo()",
    ) && domainSource.includes(
      "fn semantic_order_by_expression_appends_and_inserts_before_limit_with_undo()",
    ) && domainSource.includes(
      "fn semantic_expression_leaf_edits_create_literals_and_identifiers_with_undo()",
    ) && domainSource.includes(
      "fn semantic_expression_cast_wrapper_creates_typed_cast_with_undo()",
    ) && domainSource.includes(
      "fn semantic_expression_aggregate_wrapper_uses_registered_function_and_undo()",
    ) && domainSource.includes(
      "fn semantic_expression_nodes_can_copy_same_query_nodes_and_support_undo()",
    ) && wasmSource.includes(
      "fn sql_document_binary_expression_protocol_matches_native()",
    ) && wasmSource.includes(
      "fn sql_document_non_projection_binary_expression_protocol_matches_native()",
    ) && wasmSource.includes(
      "fn sql_document_group_by_expression_protocol_matches_native()",
    ) && wasmSource.includes(
      "fn sql_document_order_by_expression_protocol_matches_native()",
    ) && wasmSource.includes(
      "fn sql_document_expression_leaf_protocol_matches_native()",
    ) && wasmSource.includes(
      "fn sql_document_expression_cast_wrapper_protocol_matches_native()",
    ) && wasmSource.includes(
      "fn sql_document_expression_aggregate_wrapper_protocol_matches_native()",
    ) && wasmSource.includes(
      "fn sql_document_expression_copy_protocol_matches_native()",
    ) && editorTest.includes(
      'it("composes a projection from two typed expressions through the Rust protocol"',
    ) && editorTest.includes(
      'it("composes a WHERE expression from same-query typed nodes"',
    ) && editorTest.includes(
      'it("adds a typed expression to GROUP BY before later clauses"',
    ) && editorTest.includes(
      'it("adds a typed expression to ORDER BY from the query node"',
    ) && editorTest.includes(
      'it("creates a typed literal leaf from an existing expression node"',
    ) && editorTest.includes(
      'it("wraps a selected typed expression in CAST through Rust"',
    ) && editorTest.includes(
      'it("builds a registered DISTINCT aggregate around a projection expression"',
    ) && editorTest.includes(
      'it("replaces an expression from a typed node in the same query"',
    ) && frontendProtocolTest.includes(
      'it("composes a SELECT expression from typed SQL nodes in Rust"',
    ) && frontendProtocolTest.includes(
      'it("composes a non-projection expression from typed SQL nodes in Rust"',
    ) && frontendProtocolTest.includes(
      'it("adds a typed expression to GROUP BY in Rust"',
    ) && frontendProtocolTest.includes(
      'it("adds a typed expression to ORDER BY before the row limit in Rust"',
    ) && frontendProtocolTest.includes(
      'it("creates a literal leaf from an existing typed expression in Rust"',
    ) && frontendProtocolTest.includes(
      'it("wraps a typed expression in CAST through the Rust protocol"',
    ) && frontendProtocolTest.includes(
      'it("builds registered aggregate expressions through the Rust protocol"',
    ) && frontendProtocolTest.includes(
      'it("adds a typed expression to ORDER BY before the row limit in Rust"',
    ) && frontendProtocolTest.includes(
      'it("replaces any expression from another typed node in the same query"',
    ) && authorizedQueryTests.includes(
      "a constructed expression plans and runs inside the authorized Form context",
    ) && authorizedQueryTests.includes(
      "a copied typed expression reaches authorized SQL planning and execution",
    ) && authorizedQueryTests.includes(
      "the typed grouping expression reaches authorized SQL planning and execution",
    ),
    true,
    "constructed SQL expressions need Domain, WASM, UI, and authorized planner evidence",
  );
  assertEquals(
    editorTest.includes(
      'it("moves a typed projection item with one guarded semantic operation"',
    ) && editorTest.includes(
      'it("removes a typed projection item without opening a second editor"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("edits arithmetic unary signs through the Rust semantic edit protocol"',
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "async fn typed_sql_document_edits_execute_through_authorized_query_context()",
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "the exact CASE tree literal edit changes the authorized aggregate result",
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "the added typed CASE branch executes through the authorized query context",
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "a typed expression copied into COUNT executes through the authorized query context",
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "a typed projection expression replacement executes through the authorized query context",
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "a typed projection expression insertion executes through the authorized query context",
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "a typed projection reorder executes through the authorized query context",
    ) && authorizedQueryTests.includes(
      "a typed projection removal executes through the authorized query context",
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      'let unary_source = "SELECT -COUNT(*) AS row_count FROM tasks";',
    ),
    true,
  );
});

Deno.test("CTE tree edits are typed and reach authorized execution", async () => {
  const [
    domainSource,
    wasmSource,
    frontendProtocolTest,
    editorTest,
    authorizedQueryTests,
  ] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  const ctes = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "common_table_expressions");
  assertEquals(ctes.editor_coverage, "bounded");
  assertEquals(ctes.editor_operations, ["document"]);
  assertEquals(ctes.verification, [
    "domain_edit_undo",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
    "scope_resolution",
  ]);
  assertEquals(
    domainSource.includes(
      "fn semantic_cte_rename_updates_declaration_and_references_atomically()",
    ) && domainSource.includes(
      "fn semantic_cte_rename_respects_nested_shadowing_and_other_query_scopes()",
    ) && domainSource.includes(
      "fn semantic_cte_add_and_remove_respect_nested_with_scopes()",
    ) &&
      domainSource.includes(
        "fn semantic_cte_add_creates_or_extends_with_and_supports_undo()",
      ) &&
      domainSource.includes(
        "fn semantic_cte_remove_preserves_other_ctes_and_supports_undo()",
      ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_cte_rename_semantic_edit_protocol_matches_native()",
    ) &&
      wasmSource.includes(
        "fn sql_document_cte_add_and_remove_semantic_edit_protocol_matches_native()",
      ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("renames a CTE and its relation references through the Rust protocol"',
    ) && frontendProtocolTest.includes(
      'it("respects nested CTE shadowing through the Rust protocol"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("adds a CTE from the selected query node through the Rust protocol"',
    ) &&
      editorTest.includes(
        'it("removes a selected CTE through the Rust protocol"',
      ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "SqlDocumentSemanticEdit::AddCommonTableExpression",
    ) &&
      authorizedQueryTests.includes(
        "SqlDocumentSemanticEdit::RemoveCommonTableExpression",
      ) &&
      authorizedQueryTests.includes(
        "the exact SQL produced by a typed CTE addition is authorized and executable",
      ) && authorizedQueryTests.includes(
        "scope-aware CTE rename preserves nested shadowing and executes in the authorized context",
      ),
    true,
  );
});

Deno.test("SQL predicate negation is typed and reaches authorized execution", async () => {
  const [
    domainSource,
    wasmSource,
    frontendProtocolTest,
    editorTest,
    authorizedQueryTests,
  ] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  const where = matrix.editor.constructs.find((construct: { id: string }) =>
    construct.id === "where_predicate"
  );
  assertEquals(where.editor_coverage, "bounded");
  assertEquals(where.editor_operations, ["filter", "document"]);
  assertEquals(where.verification, [
    "domain_edit_undo",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSource.includes(
      "fn semantic_predicate_negation_preserves_comments_and_supports_undo()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_document_predicate_negation_semantic_edit_protocol_matches_native()",
    ),
    true,
  );
  assertEquals(
    frontendProtocolTest.includes(
      'it("toggles predicate negation through the Rust protocol and preserves comments"',
    ),
    true,
  );
  assertEquals(
    editorTest.includes(
      'it("toggles predicate negation through the Rust semantic edit protocol"',
    ),
    true,
  );
  assertEquals(
    authorizedQueryTests.includes(
      "ReplacePredicateNegation",
    ),
    true,
  );
});

Deno.test("typed WHERE parameters reach authorized execution", async () => {
  const [
    domainSource,
    wasmSource,
    frontendSource,
    protocolTest,
    authorizedSource,
  ] = await Promise.all([
    read("crates/ugoite-domain/src/sql_structure.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/components/SqlStructureToolbar.test.tsx"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  const where = matrix.editor.constructs.find((construct: { id: string }) =>
    construct.id === "where_predicate"
  );
  assertEquals(where.backend_admission, "planner_decides");
  assertEquals(where.editor_coverage, "bounded");
  assertEquals(
    matrix.editor.operations.find((operation: { action: string }) =>
      operation.action === "filter"
    )?.coverage,
    "typed column-to-parameter and same-type column-to-column comparisons for supported comparison operators, single-value IN/NOT IN, separate typed lower/upper BETWEEN bounds, LIKE/ILIKE patterns, and IS NULL/IS NOT NULL with nested AND/OR/NOT; groups are bounded to depth 32 and 128 nodes",
  );
  assertEquals(where.verification, [
    "domain_edit_undo",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_protocol_proposes_a_typed_filter_parameter()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_protocol_proposes_a_typed_column_comparison()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn typed_filter_edits_support_membership_ranges_and_string_patterns()",
    ),
    true,
  );
  assertEquals(
    domainSource.includes(
      "fn column_comparison_filters_are_typed_and_round_trip()",
    ),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("adds a typed column comparison through the filter tree"',
    ),
    true,
  );
  assertEquals(
    protocolTest.includes(
      'it("builds typed column comparisons through the Rust protocol"',
    ),
    true,
  );
  assertEquals(authorizedSource.includes("ColumnComparison"), true);
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_protocol_proposes_two_typed_between_bounds()",
    ),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("edits both typed bounds of a BETWEEN filter"',
    ),
    true,
  );
  assertEquals(
    protocolTest.includes(
      'it("round trips a BETWEEN predicate with two typed parameter bindings"',
    ),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("creates a typed argument when filtering a fresh Form query"',
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "both range bounds are bound through the authorized query context",
    ),
    true,
  );
});

Deno.test("SQL projection alias editing stays in the Rust-backed Columns contract", async () => {
  const [
    domainSql,
    wasmSource,
    toolbarSource,
    protocolSource,
    frontendTest,
    authorizedTest,
  ] = await Promise.all([
    read("crates/ugoite-domain/src/sql_structure.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/components/SqlStructureToolbar.tsx"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
    read("frontend/src/components/SqlStructureToolbar.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  const projection = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "projection");
  assertEquals(projection.backend_admission, "planner_decides");
  assertEquals(projection.editor_coverage, "bounded");
  assertEquals(projection.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSql.includes(
      "fn set_columns_replaces_only_the_projection_source_span()",
    ),
    true,
  );
  assertEquals(
    domainSql.includes(
      "fn analyzes_and_edits_simple_form_column_projection_aliases_losslessly()",
    ),
    true,
  );
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
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_projection_slots_are_proposed_through_the_wasm_protocol()",
    ),
    true,
  );
  assertEquals(
    frontendTest.includes(
      'it("edits a projection column and alias while preserving its slot id"',
    ),
    true,
  );
  assertEquals(
    authorizedTest.includes(
      "the typed Columns edit returns executable SQL",
    ),
    true,
  );
});

Deno.test("Form relation selection reaches authorized execution", async () => {
  const [domainSource, wasmSource, frontendSource, authorizedSource] =
    await Promise.all([
      read("crates/ugoite-domain/src/sql_structure.rs"),
      read("crates/ugoite-wasm/src/lib.rs"),
      read("frontend/src/components/SqlStructureToolbar.test.tsx"),
      read("crates/ugoite-iceberg/tests/authorized_query.rs"),
    ]);
  const fromRelation = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "from_relation");
  assertEquals(fromRelation.backend_admission, "planner_decides");
  assertEquals(fromRelation.editor_coverage, "bounded");
  assertEquals(fromRelation.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSource.includes(
      "fn structure_proposals_return_one_applicable_smallest_typed_node_edit()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_protocol_proposes_form_relation_selection()",
    ),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("routes Form selection through the Rust relation proposal"',
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "the trusted Form picker returns executable SQL",
    ),
    true,
  );
});

Deno.test("SQL feature matrix aggregate catalog remains present in the typed UI picker", async () => {
  const [
    frontendSql,
    domainSql,
    functionRegistry,
    clientProtocol,
    wasmSource,
    frontendTest,
    authorizedSource,
  ] = await Promise.all([
    read("frontend/src/lib/sql.ts"),
    read("crates/ugoite-domain/src/sql_structure.rs"),
    read("crates/ugoite-domain/src/sql_functions.rs"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/components/SqlStructureToolbar.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
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

  const descriptors = [...functionRegistry.matchAll(
    /const ([A-Z]+): SqlAggregateFunction = SqlAggregateFunction \{([\s\S]*?)\n\};/g,
  )].map(([, , body]) => ({
    name: body.match(/\n\s*name: "([^"]+)"/)?.[1],
    aliases: [...(body.match(/\n\s*aliases: &\[([^\]]*)\]/)?.[1] ?? "")
      .matchAll(/"([^"]+)"/g)].map(([, alias]) => alias),
  }));
  const registryFunctions = descriptors.map((descriptor) => descriptor.name);
  const registryAliases = Object.fromEntries(
    descriptors.flatMap((descriptor) =>
      descriptor.aliases.map((alias) => [alias, descriptor.name])
    ),
  );
  const frontendFunctionUnion = clientProtocol.match(
    /export type SqlStructureAggregateFunction =([\s\S]*?);/,
  )?.[1];
  const frontendFunctions = [...(frontendFunctionUnion ?? "").matchAll(
    /"([a-z_]+)"/g,
  )].map(([, name]) => name);

  const guiFunctions = [
    ...backendFunctions,
    ...Object.keys(backendAggregateAliases),
  ].sort();
  assertEquals(editorFunctions?.slice().sort(), guiFunctions);
  assertEquals(registryFunctions, backendFunctions);
  assertEquals(backendAggregateAliases, registryAliases);
  assertEquals(frontendFunctions.slice().sort(), guiFunctions);
  assertEquals(
    matrix.editor.aggregate_contract.checked,
    "registry_signature_and_gui_roundtrip",
  );
  assertEquals(
    matrix.editor.aggregate_contract.known_editor_rejections,
    [],
  );
  assertEquals(matrix.editor.aggregate_contract.verification_gaps, []);
  assertEquals(matrix.editor.aggregate_contract.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSql.includes(
      "fn aggregate_operation_serializes_every_allowlisted_function()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes("fn sql_structure_protocol_proposes_count_aggregate()"),
    true,
  );
  assertEquals(
    frontendTest.includes(
      'it("keeps COUNT(*) available while allowing COUNT(DISTINCT column)"',
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "the typed aggregate edit returns executable SQL",
    ),
    true,
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
  const [
    backendSource,
    domainSource,
    toolbarSource,
    workspaceSource,
    workspaceTestSource,
  ] = await Promise.all([
    read("crates/ugoite-iceberg/src/index.rs"),
    read("crates/ugoite-domain/src/sql_structure.rs"),
    read("frontend/src/components/SqlStructureToolbar.tsx"),
    read("frontend/src/components/SQLWorkspace.tsx"),
    read("frontend/src/components/SQLWorkspace.test.tsx"),
  ]);
  const joinConstruct = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "joins");
  assertEquals(joinConstruct.backend_admission, "planner_decides");
  assertEquals(joinConstruct.editor_coverage, "bounded");
  assertEquals(
    joinConstruct.limits.includes(
      "one self-join of the selected Form with explicit distinct aliases and a closed INNER/LEFT/RIGHT/FULL join-kind choice",
    ),
    true,
  );
  assertEquals(
    joinConstruct.limits.includes(
      "CROSS/NATURAL/USING, multiple joins, derived relations, reversed equality operands, and other ON expressions fail closed",
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
      "WHERE supports bounded AND/OR/NOT trees of qualified parameter comparisons, single-value IN/NOT IN, two-bound BETWEEN, LIKE/ILIKE, and IS NULL/IS NOT NULL on qualified columns; adding a JOIN qualifies every supported predicate leaf; ORDER BY supports simple qualified columns with ASC/DESC and NULLS FIRST/LAST; unqualified references fail closed",
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
  assertEquals(matrix.editor.toolbar.visible_actions, [
    "form",
    "columns",
    "filter",
    "sort",
    "structure",
    "arguments",
  ]);
  assertEquals(
    [
      ...matrix.editor.operations.filter((operation: {
        action: string;
        toolbar_category?: string;
      }) => operation.toolbar_category === "structure").map((operation: {
        action: string;
      }) => operation.action),
    ].sort(),
    [...matrix.editor.toolbar.structure_operations].sort(),
  );
  assertEquals(
    toolbarSource.includes(
      'action === "join" || action === "document" || action === "having"',
    ),
    true,
  );
  assertEquals(
    ["inner", "left", "right", "full"].every((kind) =>
      toolbarSource.includes(`<option value="${kind}">`)
    ),
    true,
    "the toolbar exposes exactly the closed join kinds from the matrix",
  );
  assertEquals(toolbarSource.includes('"set_row_limit"'), true);
  const structureActionsStart = workspaceSource.indexOf(
    "const structureActions =",
  );
  const structureActionsEnd = workspaceSource.indexOf(
    "const structureAction:",
    structureActionsStart + "const structureActions =".length,
  );
  const structureActionsSource = workspaceSource.slice(
    structureActionsStart,
    structureActionsEnd,
  );
  const structureOperations = [...structureActionsSource.matchAll(
    /action\.kind === "([a-z_]+)"/g,
  )].map(([, action]) => action).sort();
  const structureMenuKinds = [
    ...new Set(
      matrix.editor.toolbar.structure_operations.map((action: string) =>
        ["join", "having", "document"].includes(action) ? "structure" : action
      ),
    ),
  ].sort();
  assertEquals(
    structureOperations,
    structureMenuKinds,
  );
  assertEquals(
    workspaceSource.includes('kind: "structure"'),
    true,
  );
  assertEquals(
    workspaceTestSource.includes(
      'expect(within(primaryGroup).getAllByRole("button")).toHaveLength(6)',
    ),
    true,
  );
  assertEquals(
    workspaceTestSource.includes('name: "Structure options"'),
    true,
  );
  assertEquals(workspaceSource.includes('action.kind === "row_limit"'), true);
  const primaryActionsStart = workspaceSource.indexOf(
    "const primary = current.filter(",
  );
  const primaryActionsEnd = workspaceSource.indexOf(
    "const structure =",
    primaryActionsStart,
  );
  const primaryActionsSource = workspaceSource.slice(
    primaryActionsStart,
    primaryActionsEnd,
  );
  const primaryActions = [...primaryActionsSource.matchAll(
    /action\.kind === "([a-z_]+)"/g,
  )].map(([, action]) => action);
  const argumentAction = workspaceSource.slice(
    workspaceSource.indexOf("const argumentsAction ="),
    workspaceSource.indexOf(
      "return [...primary, ...structure, ...argumentsAction]",
    ),
  );
  assertEquals(argumentAction.includes('action.kind === "arguments"'), true);
  assertEquals(
    [...primaryActions, "structure", "arguments"],
    matrix.editor.toolbar.visible_actions,
  );
  assertEquals(
    workspaceSource.includes(
      "return [...primary, ...structure, ...argumentsAction]",
    ),
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

Deno.test("ORDER BY editor resolves aliases and handles NULL placement", async () => {
  const [domainSource, wasmSource, frontendSource, authorizedSource] =
    await Promise.all([
      read("crates/ugoite-domain/src/sql_structure.rs"),
      read("crates/ugoite-wasm/src/lib.rs"),
      read("frontend/src/components/SqlStructureToolbar.test.tsx"),
      read("crates/ugoite-iceberg/tests/authorized_query.rs"),
    ]);
  const ordering = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "ordering");
  assertEquals(ordering.backend_admission, "planner_decides");
  assertEquals(ordering.editor_coverage, "bounded");
  assertEquals(ordering.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    ordering.limits.includes(
      "unqualified identifiers resolve explicit projection aliases before same-named Form columns; the editor retains the typed source binding",
    ),
    true,
  );
  assertEquals(ordering.limits.includes("NULLS FIRST or NULLS LAST"), true);
  assertEquals(
    domainSource.includes(
      "fn sort_editor_reads_and_edits_explicit_null_placement()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_protocol_preserves_explicit_null_sort_placement()",
    ),
    true,
  );
  assertEquals(
    frontendSource.includes(
      "keeps explicit NULLS placement editable in the sort dialog",
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "fn typed_sql_document_edits_execute_through_authorized_query_context()",
    ),
    true,
  );
});

Deno.test("LIMIT and OFFSET edits reach authorized execution", async () => {
  const [domainSource, wasmSource, frontendSource, authorizedSource] =
    await Promise.all([
      read("crates/ugoite-domain/src/sql_structure.rs"),
      read("crates/ugoite-wasm/src/lib.rs"),
      read("frontend/src/components/SqlStructureToolbar.test.tsx"),
      read("crates/ugoite-iceberg/tests/authorized_query.rs"),
    ]);
  const rowLimit = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "row_limit");
  assertEquals(rowLimit.backend_admission, "planner_decides");
  assertEquals(rowLimit.editor_coverage, "bounded");
  assertEquals(rowLimit.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSource.includes(
      "fn row_limit_is_typed_editable_and_preserves_surrounding_sql()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_protocol_exposes_row_limit_analysis_and_edit()",
    ),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("edits LIMIT and OFFSET as exact decimal strings under Structure"',
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "the exact GUI-proposed LIMIT",
    ),
    true,
  );
});

Deno.test("GROUP BY edits reach authorized execution", async () => {
  const [domainSource, wasmSource, frontendSource, authorizedSource] =
    await Promise.all([
      read("crates/ugoite-domain/src/sql_structure.rs"),
      read("crates/ugoite-wasm/src/lib.rs"),
      read("frontend/src/components/SqlStructureToolbar.test.tsx"),
      read("crates/ugoite-iceberg/tests/authorized_query.rs"),
    ]);
  const grouping = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "grouping");
  assertEquals(grouping.backend_admission, "planner_decides");
  assertEquals(grouping.editor_coverage, "bounded");
  assertEquals(grouping.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSource.includes(
      "fn proposes_aggregate_group_by_and_select_distinct_as_source_preserving_edits()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes("fn sql_structure_protocol_proposes_group_by_edit()"),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("proposes only capability-approved GROUP BY keys and SELECT DISTINCT"',
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "the typed GROUP BY edit returns executable SQL",
    ),
    true,
  );
});

Deno.test("HAVING edits reach authorized execution", async () => {
  const [domainSource, wasmSource, frontendSource, authorizedSource] =
    await Promise.all([
      read("crates/ugoite-domain/src/sql_structure.rs"),
      read("crates/ugoite-wasm/src/lib.rs"),
      read("frontend/src/components/SqlStructureToolbar.test.tsx"),
      read("crates/ugoite-iceberg/tests/authorized_query.rs"),
    ]);
  const having = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "having");
  assertEquals(having.backend_admission, "planner_decides");
  assertEquals(having.editor_coverage, "bounded");
  assertEquals(having.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSource.includes(
      "fn having_edit_preserves_unrelated_source_comments_and_emits_typed_node_edit()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes(
      "fn sql_structure_having_protocol_matches_domain_round_trip()",
    ),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("adds a typed HAVING aggregate comparison from the Structure action"',
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "the typed HAVING edit returns executable SQL",
    ),
    true,
  );
});

Deno.test("typed JOIN edits reach authorized execution", async () => {
  const [domainSource, wasmSource, frontendSource, authorizedSource] =
    await Promise.all([
      read("crates/ugoite-domain/src/sql_structure.rs"),
      read("crates/ugoite-wasm/src/lib.rs"),
      read("frontend/src/components/SqlStructureToolbar.test.tsx"),
      read("crates/ugoite-iceberg/tests/authorized_query.rs"),
    ]);
  const joins = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "joins");
  assertEquals(joins.backend_admission, "planner_decides");
  assertEquals(joins.editor_coverage, "bounded");
  assertEquals(joins.verification, [
    "domain_round_trip",
    "wasm_protocol",
    "frontend_protocol",
    "authorized_executor",
  ]);
  assertEquals(
    domainSource.includes(
      "fn adds_a_bounded_self_join_and_preserves_trailing_clauses()",
    ),
    true,
  );
  assertEquals(
    wasmSource.includes("fn sql_structure_protocol_proposes_join_edit()"),
    true,
  );
  assertEquals(
    frontendSource.includes(
      'it("adds a typed self-join with a selected join kind through the shared toolbar"',
    ),
    true,
  );
  assertEquals(
    authorizedSource.includes(
      "the typed JOIN edit returns executable SQL",
    ),
    true,
  );
});

Deno.test("SQL Document CASE wrapper has cross-layer and authorized-execution evidence", async () => {
  const [domain, wasm, protocol, editor, authorized] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  assertEquals(
    domain.includes(
      "fn semantic_expression_case_wrapper_creates_editable_searched_case_with_undo()",
    ) && wasm.includes(
      "fn sql_document_expression_case_wrapper_protocol_matches_native()",
    ) && protocol.includes(
      'it("wraps a typed expression in an editable searched CASE in Rust"',
    ) && editor.includes(
      'it("wraps a typed expression in an editable searched CASE"',
    ) && authorized.includes(
      "the CASE expression preserves the numeric Form column's result type",
    ),
    true,
    "CASE wrapper edits need Domain, WASM, UI, and authorized executor evidence",
  );
});

Deno.test("SQL Document WHERE seed has cross-layer and authorized-execution evidence", async () => {
  const [domain, wasm, protocol, editor, authorized] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  assertEquals(
    domain.includes(
      "fn semantic_where_clause_seed_is_added_before_later_clauses_and_is_undoable()",
    ) && wasm.includes(
      "fn sql_document_add_where_clause_protocol_matches_native()",
    ) && protocol.includes(
      'it("adds WHERE before GROUP BY, ORDER BY, and LIMIT through Rust"',
    ) && editor.includes(
      'it("adds WHERE from a SELECT node before later clauses"',
    ) && authorized.includes(
      "the generated WHERE seed remains executable through the authorized query context",
    ),
    true,
    "WHERE clause creation needs Domain, WASM, UI, and authorized executor evidence",
  );
});

Deno.test("SQL Document predicate templates have cross-layer and authorized-execution evidence", async () => {
  const [domain, wasm, protocol, editor, authorized] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  assertEquals(
    domain.includes(
      "fn semantic_predicate_templates_create_editable_typed_operands_and_undo()",
    ) && wasm.includes(
      "fn sql_document_predicate_template_protocol_matches_native()",
    ) && protocol.includes(
      'it("builds BETWEEN with independently editable bounds through Rust"',
    ) && editor.includes(
      'it("builds a typed BETWEEN predicate from an expression node"',
    ) && authorized.includes(
      "the generated BETWEEN predicate is typed and authorized for execution",
    ),
    true,
    "predicate templates need Domain, WASM, UI, and authorized executor evidence",
  );
});

Deno.test("SQL Document IN-list edits have cross-layer and authorized-execution evidence", async () => {
  const [domain, wasm, protocol, editor, authorized] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  assertEquals(
    domain.includes(
      "fn semantic_in_list_values_append_remove_and_undo_preserve_surrounding_source()",
    ) && domain.includes(
      "fn semantic_in_list_edits_reject_single_value_removal_and_comments_inside_list()",
    ) && wasm.includes(
      "fn sql_document_in_list_value_protocol_matches_native()",
    ) && protocol.includes(
      'it("appends and removes typed values in IN lists through Rust"',
    ) && editor.includes(
      'it("adds an IN list value from its predicate node"',
    ) && authorized.includes(
      "the typed IN-list edit keeps execution within the authorized Form path",
    ),
    true,
    "IN-list editing requires Domain, WASM, UI, and authorized executor evidence",
  );
});

Deno.test("SQL Document scalar subquery seeds have cross-layer and planner evidence", async () => {
  const [domain, wasm, protocol, editor, authorized] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.test.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.test.tsx"),
    read("crates/ugoite-iceberg/tests/authorized_query.rs"),
  ]);
  const projection = matrix.editor.constructs.find((construct: {
    id: string;
  }) => construct.id === "projection");
  assertEquals(
    projection.limits.some((limit: string) =>
      limit.includes("typed SQL-tree operation appends a relation-free `(SELECT 1)` scalar subquery projection")
    ),
    true,
  );
  assertEquals(
    domain.includes(
      "fn semantic_scalar_subquery_projection_is_typed_parseable_and_undoable()",
    ) && wasm.includes(
      "fn sql_document_scalar_subquery_projection_protocol_matches_native()",
    ) && protocol.includes(
      'it("adds a relation-free scalar subquery projection through Rust"',
    ) && editor.includes(
      'it("adds a scalar subquery projection through the Rust protocol"',
    ) && authorized.includes(
      "a constructed scalar subquery reaches authorized planning and execution",
    ),
    true,
    "scalar subquery construction requires Domain, WASM, UI, and authorized planner evidence",
  );
});
