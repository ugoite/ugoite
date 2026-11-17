import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);
const read = (path: string) => Deno.readTextFile(new URL(path, root));

async function sourceFiles(directory: string): Promise<string[]> {
  const files: string[] = [];
  for await (const entry of Deno.readDir(new URL(directory, root))) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory) files.push(...await sourceFiles(path));
    else if (/\.(?:ts|tsx)$/.test(entry.name)) files.push(path);
  }
  return files;
}

Deno.test("retired SQL structure UI has no production source or import", async () => {
  const retired = [
    "SqlStructureToolbar",
    "SqlDocumentTreeEditor",
    "SqlColumnsEditor",
    "SqlFilterSortEditor",
    "SqlConditionWorkspace",
    "SqlHavingExpressionEditor",
  ];
  for (const name of retired) {
    await read(`frontend/src/components/${name}.tsx`).then(
      () => {
        throw new Error(`${name} is still present`);
      },
      () => undefined,
    );
  }
  const sources = await Promise.all((await sourceFiles("frontend/src"))
    .map(async (path) => [path, await read(path)] as const));
  const references = sources.flatMap(([path, source]) =>
    retired.filter((name) => source.includes(name)).map((name) => ({
      path,
      name,
    }))
  );
  assertEquals(references, []);
  for (
    const path of [
      "e2e/sql-condition-workspace.test.ts",
      "e2e/sql-columns-editor.test.ts",
      "e2e/composition-filter-conditions.test.ts",
      "docs/mitase/requirements/sql/task-filter-sort.yaml",
    ]
  ) {
    await read(path).then(
      () => {
        throw new Error(`${path} still requires the retired SQL GUI`);
      },
      () => undefined,
    );
  }
});

Deno.test("published SQL Document native and WASM contracts remain wired", async () => {
  const [domain, wasm, protocol, fixtures, mise] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
    read("crates/ugoite-wasm/tests/sql_structure_wasm_fixtures.ts"),
    read("mise.toml"),
  ]);
  for (
    const symbol of [
      "parse_sql_document",
      "rename_sql_document_parameter",
      "apply_sql_document_semantic_edit_transaction",
    ]
  ) assertEquals(domain.includes(`fn ${symbol}`), true, symbol);
  for (
    const action of [
      "domain.parse_sql_document",
      "domain.rename_sql_document_parameter",
      "domain.apply_sql_document_semantic_edit_transaction",
    ]
  ) {
    assertEquals(wasm.includes(`\"${action}\"`), true, action);
    assertEquals(protocol.includes(`\"${action}\"`), true, action);
  }
  assertEquals(fixtures.includes("sql_document"), true);
  assertEquals(mise.includes("check:wasm:sql-structure-fixtures"), true);
  assertEquals(
    domain.includes(
      "lossless_document_retains_source_tokens_and_syntax_ranges_without_execution_admission",
    ),
    true,
  );
  assertEquals(
    wasm.includes(
      "sql_document_protocol_matches_native_parser_for_draft_states_and_ranges",
    ),
    true,
  );
});

Deno.test("shared parser parity corpus remains a backend parser contract", async () => {
  const [fixture, domain, runtime, historical] = await Promise.all([
    read("shared/sql/sql-document-parser-parity.json").then(JSON.parse),
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-iceberg/src/sql_parser.rs"),
    read("shared/sql/ugoite-sql-rules.json").then(JSON.parse),
  ]);
  assertEquals(historical.feature_matrix.status, "historical");
  assertEquals(fixture.cases.length >= 13, true);
  assertEquals(
    domain.includes(
      "shared_parser_parity_cases_are_parsed_by_the_sql_document",
    ),
    true,
  );
  assertEquals(
    runtime.includes(
      "shared_parser_parity_cases_are_accepted_by_datafusion_and_sql_document",
    ),
    true,
  );
});
