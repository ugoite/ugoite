import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);
const read = (path: string) => Deno.readTextFile(new URL(path, root));

Deno.test("current SQL Document native and WASM contracts remain wired", async () => {
  const [domain, wasm, protocol] = await Promise.all([
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-wasm/src/lib.rs"),
    read("frontend/src/lib/ugoite-client/protocol.ts"),
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
  const [fixture, domain, runtime] = await Promise.all([
    read("shared/sql/sql-document-parser-parity.json").then(JSON.parse),
    read("crates/ugoite-domain/src/sql_document.rs"),
    read("crates/ugoite-iceberg/src/sql_parser.rs"),
  ]);
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
