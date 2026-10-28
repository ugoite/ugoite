import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url).pathname;
const read = (path: string) => Deno.readTextFile(`${root}${path}`);
const matrix =
  JSON.parse(await read("shared/sql/ugoite-sql-rules.json")).feature_matrix;
const snake = (name: string) =>
  name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();
const rustVariants = (source: string, name: string) => {
  const body = source.match(
    new RegExp(`pub enum ${name}\\s*\\{([\\s\\S]*?)\\n\\}`),
  )?.[1];
  if (!body) throw new Error(`missing Rust enum ${name}`);
  return [...body.matchAll(/^ {4}([A-Z][A-Za-z0-9_]*)\s*(?:\{|,)/gm)].map((
    [, name],
  ) => snake(name));
};
const tsKinds = (source: string, name: string, next: string) => {
  const start = source.indexOf(`export type ${name} =`);
  const end = source.indexOf(`export type ${next} =`, start);
  if (start < 0 || end < 0) throw new Error(`missing TypeScript union ${name}`);
  const body = source.slice(start, end);
  const kinds: string[] = [];
  let depth = 0;
  for (let index = 0; index < body.length; index += 1) {
    const character = body[index];
    if (character === "{") {
      depth += 1;
      continue;
    }
    if (character === "}") {
      depth -= 1;
      continue;
    }
    if (
      depth === 1 && body.startsWith("kind:", index) &&
      (index === 0 || !/[A-Za-z0-9_$]/.test(body[index - 1]))
    ) {
      const kind = body.slice(index).match(/^kind:\s*"([a-z_]+)"/)?.[1];
      if (kind) kinds.push(kind);
    }
  }
  return kinds;
};
const sameSet = (actual: string[], expected: string[]) => {
  assertEquals(
    new Set(actual).size,
    actual.length,
    "inventory entries must be unique",
  );
  assertEquals([...actual].sort(), [...expected].sort());
};

Deno.test("SQL matrix pins the read-only gate, both parser versions and dialect", async () => {
  const [backend, parser, domain, manifest, lock, query] = await Promise.all([
    read("crates/ugoite-iceberg/src/index.rs"),
    read("crates/ugoite-iceberg/src/sql_parser.rs"),
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-domain/Cargo.toml"),
    read("Cargo.lock"),
    read("crates/ugoite-iceberg/src/query_context.rs"),
  ]);
  assertEquals(
    matrix.backend.statement_gate,
    "exactly_one_select_query_statement",
  );
  assertEquals(matrix.backend.closed_clause_allowlist, false);
  assertEquals(matrix.backend.query_body_policy.allowed_set_expr_kinds, [
    "select",
    "query",
    "set_operation",
  ]);
  assertEquals(matrix.backend.query_body_policy.unknown_set_expr, "reject");
  assertEquals(
    matrix.backend.parser_engine.version,
    lock.match(/^name = "datafusion"\nversion = "([^"]+)"/m)?.[1],
  );
  assertEquals(
    matrix.editor.source_parser.version,
    manifest.match(/^sqlparser = \{ version = "([^"]+)"/m)?.[1],
  );
  assertEquals(
    lock.includes(
      `name = "sqlparser"\nversion = "${matrix.editor.source_parser.version}"`,
    ),
    true,
  );
  assertEquals(matrix.backend.parser_dialect, "GenericDialect");
  assertEquals(matrix.editor.parser_dialect, matrix.backend.parser_dialect);
  assertEquals(
    domain.includes("use sqlparser::dialect::GenericDialect;"),
    true,
  );
  assertEquals(
    parser.includes("SQL_PARSER_RECURSION_LIMIT: usize = 256"),
    true,
  );
  assertEquals(
    domain.includes("SQL_DOCUMENT_PARSER_RECURSION_LIMIT: usize = 256"),
    true,
  );
  assertEquals(parser.includes("with_recursion_limit("), true);
  assertEquals(query.includes('"datafusion.sql_parser.recursion_limit"'), true);
  const validator = backend.slice(
    backend.indexOf("fn set_expr_is_select_only("),
    backend.indexOf("struct SelectBodyValidator"),
  );
  sameSet(
    [...validator.matchAll(
      /SetExpr::([A-Za-z0-9_]+)\s*(?:\([^)]*\)|\{[^}]*\})?\s*=>/g,
    )].map(([, name]) => snake(name)),
    matrix.backend.query_body_policy.allowed_set_expr_kinds,
  );
  assertEquals(validator.includes("_ => false"), true);
  assertEquals(
    backend.includes("query_contains_only_select_bodies(query)"),
    true,
  );
});

Deno.test("executor-admitted constructs map to the shared typed SQL document", async () => {
  const [domain, protocol] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
  ]);
  const contract = matrix.editor.typed_document_contract;
  assertEquals(
    contract.version,
    Number(
      domain.match(/SQL_DOCUMENT_CONTRACT_VERSION:\s*u32\s*=\s*(\d+)/)?.[1],
    ),
  );
  assertEquals(contract.parser_engine, matrix.backend.parser_engine);
  sameSet(contract.node_kinds, rustVariants(domain, "SqlDocumentNodeKind"));
  const union =
    protocol.match(/export type SqlDocumentNodeKind\s*=([\s\S]*?);/)?.[1] ?? "";
  sameSet(
    contract.node_kinds,
    [...union.matchAll(/"([a-z_]+)"/g)].map(([, name]) => name),
  );
  sameSet(contract.semantic_kinds, rustVariants(domain, "SqlDocumentNodeData"));
  sameSet(
    contract.semantic_kinds,
    tsKinds(protocol, "SqlDocumentNodeData", "SqlDocumentNode"),
  );
  const represented = new Set<string>(contract.statement_nodes);
  const ids: string[] = [];
  for (const construct of matrix.editor.constructs) {
    ids.push(construct.id);
    if (construct.backend_admission === "planner_decides") {
      assertEquals(construct.document_handling, "typed_tree");
      assertEquals(construct.document_nodes.length > 0, true);
      assertEquals(construct.limits.length > 0, true);
    } else {
      assertEquals(construct.backend_admission, "statement_gate_rejects");
      assertEquals(construct.document_handling, "rejected_by_executor_gate");
      assertEquals(construct.document_nodes, []);
    }
    assertEquals(
      ["bounded", "context_only", "preserved_only", "none"].includes(
        construct.editor_coverage,
      ),
      true,
    );
    for (const kind of construct.document_nodes) {
      assertEquals(contract.node_kinds.includes(kind), true);
      represented.add(kind);
    }
  }
  sameSet([...represented], contract.node_kinds);
  assertEquals(new Set(ids).size, ids.length);
  assertEquals(matrix.editor.coverage_complete, false);
  assertEquals(matrix.editor.verification_gaps.length > 0, true);
});

Deno.test("every typed semantic operation is inventoried in Rust, TypeScript and the current GUI", async () => {
  const [
    domain,
    protocol,
    editor,
    filterSortEditor,
    conditionWorkspace,
    toolbar,
  ] = await Promise
    .all([
      read("crates/ugoite-domain/src/sql_document.rs"),
      read("frontend/src/lib/ugoite-client/protocol.ts"),
      read("frontend/src/components/SqlDocumentTreeEditor.tsx"),
      read("frontend/src/components/SqlFilterSortEditor.tsx"),
      read("frontend/src/components/SqlConditionWorkspace.tsx"),
      read("frontend/src/components/SqlStructureToolbar.tsx"),
    ]);
  sameSet(
    matrix.editor.semantic_operations,
    rustVariants(domain, "SqlDocumentSemanticEdit"),
  );
  sameSet(
    matrix.editor.semantic_operations,
    tsKinds(
      protocol,
      "SqlDocumentSemanticEdit",
      "SqlDocumentSemanticEditTransaction",
    ),
  );
  assertEquals(matrix.editor.active_engine, "sql_document");
  for (const operation of matrix.editor.semantic_operations) {
    assertEquals(
      (editor + filterSortEditor + conditionWorkspace).includes(
        `"${operation}"`,
      ),
      true,
      `${operation} must be reachable from the current inspector`,
    );
  }
  const actions = toolbar.match(/type StructureAction\s*=([\s\S]*?);/)?.[1] ??
    "";
  sameSet(
    matrix.editor.operations.map((operation: { action: string }) =>
      operation.action
    ),
    [...actions.matchAll(/"([a-z_]+)"/g)].map(([, name]) => name),
  );
  assertEquals(
    matrix.editor.operations.every((operation: { protocol_family: string }) =>
      operation.protocol_family === "sql_document"
    ),
    true,
  );
  assertEquals(toolbar.includes("SqlDocumentTreeEditor"), true);
  for (const old of ["analyzeSqlStructure", "proposeSqlStructureOperation"]) {
    assertEquals(toolbar.includes(old) || editor.includes(old), false);
  }
});

Deno.test("runtime and editor parsers share a checked grammar fixture corpus", async () => {
  const path = "shared/sql/sql-document-parser-parity.json";
  const [fixture, domain, parser] = await Promise.all([
    read(path).then(JSON.parse),
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-iceberg/src/sql_parser.rs"),
  ]);
  assertEquals(matrix.backend.parser_parity_fixture, path);
  assertEquals(matrix.editor.parser_parity_fixture, path);
  assertEquals(
    new Set(fixture.cases.map((item: { id: string }) => item.id)).size,
    fixture.cases.length,
  );
  assertEquals(fixture.cases.length >= 13, true);
  assertEquals(
    domain.includes(
      "fn shared_parser_parity_cases_are_parsed_by_the_sql_document()",
    ),
    true,
  );
  assertEquals(
    parser.includes(
      "fn shared_parser_parity_cases_are_accepted_by_datafusion_and_sql_document()",
    ),
    true,
  );
  assertEquals(domain.includes(path) && parser.includes(path), true);
});

Deno.test("active aggregate picker follows only the enabled runtime registry", async () => {
  const [registry, protocol, editor, core] = await Promise.all([
    read("crates/ugoite-domain/src/sql_functions.rs"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
    read("frontend/src/components/SqlDocumentTreeEditor.tsx"),
    read("crates/ugoite-core/src/query.rs"),
  ]);
  const enabled =
    registry.match(/pub const USER_SQL_AGGREGATES:[^=]*=\s*&\[([^\]]+)\]/)
      ?.[1] ?? "";
  const names: string[] = [];
  const aliases: Record<string, string> = {};
  for (const id of enabled.match(/[A-Z][A-Z_]+/g) ?? []) {
    const body = registry.match(
      new RegExp(
        `const ${id}: SqlAggregateFunction = SqlAggregateFunction \\{([\\s\\S]*?)\\n\\};`,
      ),
    )?.[1];
    if (!body) throw new Error(`missing enabled descriptor ${id}`);
    const name = body.match(/name: "([^"]+)"/)?.[1];
    if (!name) throw new Error(`missing aggregate name ${id}`);
    names.push(name);
    for (
      const [, alias] of (body.match(/aliases: &\[([^\]]*)\]/)?.[1] ?? "")
        .matchAll(/"([^"]+)"/g)
    ) aliases[alias] = name;
  }
  assertEquals(names, matrix.backend.functions.aggregate);
  assertEquals(aliases, matrix.backend.function_aliases.aggregate);
  const union =
    protocol.match(/export type SqlDocumentAggregateFunction\s*=([\s\S]*?);/)
      ?.[1] ?? "";
  sameSet([...union.matchAll(/"([a-z_]+)"/g)].map(([, name]) => name), names);
  for (const name of names) assertEquals(editor.includes(`"${name}"`), true);
  assertEquals(
    core.includes(
      "fn shared_sql_feature_matrix_matches_read_only_runtime_function_policy()",
    ),
    true,
  );
});

Deno.test("document evidence points to current executable tests without claiming this inventory executes them", async () => {
  for (const entry of matrix.editor.evidence) {
    for (const [path, selector] of entry.selectors as [string, string][]) {
      const source = await read(path);
      assertEquals(
        source.includes(selector),
        true,
        `${entry.id}: stale selector ${selector} in ${path}`,
      );
    }
  }
  const [tasks, workflow] = await Promise.all([
    read("mise.toml"),
    read(".github/workflows/ci.yml"),
  ]);
  assertEquals(tasks.includes("deno test -A tools/*_test.ts"), true);
  assertEquals(tasks.includes("cargo nextest run --workspace --locked"), true);
  assertEquals(tasks.includes('"test:frontend:coverage"'), true);
  assertEquals(workflow.includes("merge_group:"), true);
  assertEquals(workflow.includes("mise run ci:lane:web"), true);
  assertEquals(workflow.includes("mise run ci:lane:rust-test"), true);
});
