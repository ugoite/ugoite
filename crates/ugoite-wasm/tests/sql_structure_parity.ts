const workspaceRoot = new URL("../../..", import.meta.url).pathname;
const MAX_SQL_DOCUMENT_BYTES = 256 * 1024;

async function runCommand(command: string, args: string[]): Promise<string> {
  const result = await new Deno.Command(command, {
    args,
    cwd: workspaceRoot,
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (result.code !== 0) {
    throw new Error(
      new TextDecoder().decode(result.stderr) ||
        `${command} exited with status ${result.code}`,
    );
  }
  return new TextDecoder().decode(result.stdout);
}

function assertEqual(actual: unknown, expected: unknown, label: string): void {
  if (actual !== expected) {
    throw new Error(
      `${label}: expected ${String(expected)}, got ${String(actual)}`,
    );
  }
}

function sourceSliceByByteRange(
  source: string,
  range: { start: number; end: number },
): string {
  const bytes = new TextEncoder().encode(source);
  if (
    !Number.isInteger(range.start) || !Number.isInteger(range.end) ||
    range.start < 0 || range.end < range.start || range.end > bytes.length
  ) {
    throw new Error(`Invalid SQL source range ${JSON.stringify(range)}`);
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(
    bytes.subarray(range.start, range.end),
  );
}

function assertDocumentRanges(
  document: Record<string, unknown>,
  source: string,
  label: string,
): void {
  assertEqual(document.source, source, `${label} exact source`);
  const tokens = document.tokens as Record<string, unknown>[];
  const nodes = document.nodes as Record<string, unknown>[];
  const sourceBytes = new TextEncoder().encode(source);
  if (sourceBytes.length > MAX_SQL_DOCUMENT_BYTES) {
    assertEqual(tokens.length, 0, `${label} oversized token list`);
    assertEqual(nodes.length, 0, `${label} oversized node list`);
    return;
  }

  let nextByte = 0;
  for (const [index, token] of tokens.entries()) {
    const range = token.range as { start: number; end: number };
    assertEqual(range.start, nextByte, `${label} token ${index} start`);
    sourceSliceByByteRange(source, range);
    nextByte = range.end;
  }
  assertEqual(
    nextByte,
    sourceBytes.length,
    `${label} token coverage in UTF-8 bytes`,
  );
  for (const [index, node] of nodes.entries()) {
    const range = node.range as { start: number; end: number } | null;
    if (range !== null) sourceSliceByByteRange(source, range);
    assertEqual(node.id, index, `${label} node index`);
  }
}

async function invokeWasm(
  exports: WebAssembly.Exports,
  request: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const memory = exports.memory as WebAssembly.Memory;
  const allocate = exports.ugoite_alloc as (length: number) => number;
  const deallocate = exports.ugoite_dealloc as (
    pointer: number,
    length: number,
  ) => void;
  const invoke = exports.ugoite_protocol_invoke as (
    pointer: number,
    length: number,
  ) => number;
  const resultPointer = exports.ugoite_protocol_result_pointer as () => number;
  const resultLength = exports.ugoite_protocol_result_length as () => number;
  const clearResult = exports.ugoite_protocol_clear_result as () => void;

  const input = new TextEncoder().encode(JSON.stringify(request));
  const pointer = allocate(input.length);
  new Uint8Array(memory.buffer, pointer, input.length).set(input);
  const status = invoke(pointer, input.length);
  deallocate(pointer, input.length);
  if (status !== 0) {
    throw new Error(`WASM protocol invocation failed with status ${status}`);
  }

  const output = new Uint8Array(memory.buffer, resultPointer(), resultLength());
  const response = JSON.parse(new TextDecoder().decode(output)) as Record<
    string,
    unknown
  >;
  clearResult();
  return response;
}

async function main(): Promise<void> {
  await runCommand("cargo", [
    "build",
    "-p",
    "ugoite-wasm",
    "--target",
    "wasm32-unknown-unknown",
    "--locked",
  ]);
  const metadata = JSON.parse(
    await runCommand("cargo", [
      "metadata",
      "--no-deps",
      "--format-version",
      "1",
    ]),
  ) as { target_directory: string };
  const wasmPath =
    `${metadata.target_directory}/wasm32-unknown-unknown/debug/ugoite_wasm.wasm`;
  const wasmBytes = await Deno.readFile(wasmPath);
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});

  const documentCases = [
    {
      label: "parsed",
      sql: '-- 日付\nSELECT "日付", $day FROM "G" WHERE note = \'$day\'',
      status: "parsed",
    },
    { label: "invalid", sql: "SELECT (", status: "invalid_sql" },
    {
      label: "multiple statements",
      sql: "SELECT 1; SELECT 2",
      status: "multiple_statements",
    },
    {
      label: "maximum source with escaped trivia",
      sql: `SELECT 1 /*${"\n".repeat(MAX_SQL_DOCUMENT_BYTES - 13)}*/`,
      status: "parsed",
    },
    {
      label: "oversized",
      sql: "x".repeat(MAX_SQL_DOCUMENT_BYTES + 1),
      status: "too_large",
    },
  ];
  for (const testCase of documentCases) {
    const response = await invokeWasm(instance.exports, {
      action: "domain.parse_sql_document",
      value: { sql: testCase.sql },
    });
    assertEqual(response.ok, true, `${testCase.label} document envelope`);
    const document = response.value as Record<string, unknown>;
    assertEqual(document.status, testCase.status, `${testCase.label} status`);
    assertDocumentRanges(document, testCase.sql, testCase.label);
  }
  const parsedDocumentResponse = await invokeWasm(instance.exports, {
    action: "domain.parse_sql_document",
    value: {
      sql: documentCases[0].sql,
    },
  });
  const parsedDocument = parsedDocumentResponse.value as Record<
    string,
    unknown
  >;
  const parsedTokens = parsedDocument.tokens as Record<string, unknown>[];
  const placeholder = parsedTokens.find((token) =>
    sourceSliceByByteRange(
      documentCases[0].sql,
      token.range as { start: number; end: number },
    ) === "$day"
  );
  if (!placeholder) throw new Error("Parsed SQL placeholder token is missing");
  const placeholderRange = placeholder.range as { start: number; end: number };
  assertEqual(placeholderRange.start, 27, "placeholder UTF-8 byte start");
  assertEqual(placeholderRange.end, 31, "placeholder UTF-8 byte end");

  const amplifiedSql = `SELECT ${Array(20_000).fill("1").join(",")}`;
  const amplifiedResponse = await invokeWasm(instance.exports, {
    action: "domain.parse_sql_document",
    value: { sql: amplifiedSql },
  });
  assertEqual(amplifiedResponse.ok, false, "amplified document response");
  const amplifiedError = amplifiedResponse.error as Record<string, unknown>;
  assertEqual(
    amplifiedError.kind,
    "domain_output_too_large",
    "amplified document output bound",
  );
  if (
    new TextEncoder().encode(JSON.stringify(amplifiedResponse)).length >
      1024 * 1024
  ) {
    throw new Error("Oversized SQL Document output was not bounded");
  }

  const { instance: memoryGuardInstance } = await WebAssembly.instantiate(
    wasmBytes,
    {},
  );
  const memoryGuard = memoryGuardInstance.exports.memory as WebAssembly.Memory;
  const initialMemoryBytes = memoryGuard.buffer.byteLength;
  const nearLimitMalformedSql = "(".repeat(MAX_SQL_DOCUMENT_BYTES);
  const nearLimitMalformedResponse = await invokeWasm(
    memoryGuardInstance.exports,
    {
      action: "domain.parse_sql_document",
      value: { sql: nearLimitMalformedSql },
    },
  );
  assertEqual(
    nearLimitMalformedResponse.ok,
    true,
    "near-limit malformed SQL response envelope",
  );
  const nearLimitMalformedDocument = nearLimitMalformedResponse.value as Record<
    string,
    unknown
  >;
  assertEqual(
    nearLimitMalformedDocument.status,
    "too_complex",
    "near-limit malformed SQL complexity limit",
  );
  const memoryGrowthBytes = memoryGuard.buffer.byteLength - initialMemoryBytes;
  if (memoryGrowthBytes > 64 * 1024 * 1024) {
    throw new Error(
      `Near-limit malformed SQL grew WASM memory by ${memoryGrowthBytes} bytes`,
    );
  }

  const context = {
    relation: "Task",
    columns: [
      { name: "_ugoite_id", type: "string" },
      { name: "title", type: "string" },
      { name: "amount", type: "integer" },
    ],
    variables: [
      { name: "title", type: "string" },
      { name: "minimum", type: "integer" },
    ],
    stable_key: ["_ugoite_id"],
  };
  const sql =
    'SELECT * FROM "Task" WHERE "title" = $title ORDER BY "amount" DESC';

  const analysis = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: { sql, context },
  });
  assertEqual(analysis.ok, true, "analysis protocol envelope");
  const analyzed = analysis.value as Record<string, unknown>;
  assertEqual(analyzed.status, "supported", "analysis status");
  assertEqual(analyzed.stable_pagination, false, "stable pagination state");
  const capabilities = analyzed.capabilities as Record<string, unknown>;
  const pagination = capabilities.stable_pagination as Record<string, unknown>;
  assertEqual(pagination.enabled, false, "stable pagination capability");
  assertEqual(pagination.reason, "stable_order_required", "pagination reason");

  const proposal = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql,
      context,
      operation: {
        kind: "set_filters",
        filters: [{
          column: "amount",
          operator: "gt_eq",
          parameter: "minimum",
        }],
      },
    },
  });
  assertEqual(proposal.ok, true, "proposal protocol envelope");
  const proposed = proposal.value as Record<string, unknown>;
  assertEqual(proposed.status, "supported", "proposal status");
  assertEqual(proposed.base_sql, sql, "proposal source preservation");
  assertEqual(
    proposed.requires_confirmation,
    true,
    "explicit diff confirmation",
  );
  const proposedSql = String(proposed.proposed_sql);
  if (!proposedSql.includes('"amount" >= $minimum')) {
    throw new Error(`Typed filter parameter was not preserved: ${proposedSql}`);
  }

  const sorted = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql: 'SELECT * FROM "Task"',
      context,
      operation: {
        kind: "set_sort",
        order_by: [{ column: "amount", direction: "desc" }],
      },
    },
  });
  const sortedProposal = sorted.value as Record<string, unknown>;
  const sortedSql = String(sortedProposal.proposed_sql);
  if (!sortedSql.includes('"_ugoite_id" ASC')) {
    throw new Error(`Stable tie-breaker was not appended: ${sortedSql}`);
  }
  assertEqual(
    sortedProposal.requires_confirmation,
    true,
    "sort diff confirmation",
  );

  const aggregateSql =
    'SELECT DISTINCT "title", COUNT(DISTINCT "amount") AS unique_count FROM "Task" GROUP BY "title"';
  const aggregateAnalysis = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: { sql: aggregateSql, context },
  });
  const aggregateValue = aggregateAnalysis.value as Record<string, unknown>;
  assertEqual(
    aggregateValue.status,
    "supported",
    "aggregate/group analysis status",
  );
  assertEqual(aggregateValue.distinct, true, "SELECT DISTINCT analysis");
  assertEqual(
    JSON.stringify(aggregateValue.group_by),
    JSON.stringify(["title"]),
    "GROUP BY analysis",
  );
  const aggregates = aggregateValue.aggregates as Record<string, unknown>[];
  assertEqual(aggregates.length, 1, "aggregate count");
  assertEqual(aggregates[0].function, "count", "aggregate function");
  assertEqual(aggregates[0].column, "amount", "aggregate column");
  assertEqual(aggregates[0].distinct, true, "DISTINCT aggregate");
  const alias = aggregates[0].alias as Record<string, unknown>;
  assertEqual(alias.name, "unique_count", "aggregate alias");
  assertEqual(alias.quoted, false, "aggregate alias quoting");

  const groupedAggregate = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql: 'SELECT "title" FROM "Task" GROUP BY "title"',
      context,
      operation: {
        kind: "set_aggregates",
        aggregates: [{
          function: "sum",
          column: "amount",
          distinct: true,
          alias: { name: "total", quoted: true },
        }],
      },
    },
  });
  const aggregateProposal = groupedAggregate.value as Record<string, unknown>;
  assertEqual(
    aggregateProposal.status,
    "supported",
    "aggregate proposal status",
  );
  if (
    !String(aggregateProposal.proposed_sql).includes(
      'SUM(DISTINCT "amount") AS "total"',
    )
  ) {
    throw new Error(
      `DISTINCT aggregate rewrite was not preserved: ${aggregateProposal.proposed_sql}`,
    );
  }

  const selectDistinct = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql: 'SELECT "title" FROM "Task"',
      context,
      operation: { kind: "set_distinct", distinct: true },
    },
  });
  const distinctProposal = selectDistinct.value as Record<string, unknown>;
  assertEqual(
    distinctProposal.status,
    "supported",
    "SELECT DISTINCT proposal status",
  );
  if (!String(distinctProposal.proposed_sql).startsWith("SELECT DISTINCT")) {
    throw new Error(
      `SELECT DISTINCT rewrite was not emitted: ${distinctProposal.proposed_sql}`,
    );
  }

  const incompatibleDistinctSql =
    'SELECT "title" FROM "Task" ORDER BY "_ugoite_id"';
  const incompatibleDistinct = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql: incompatibleDistinctSql,
      context,
      operation: { kind: "set_distinct", distinct: true },
    },
  });
  const rejectedDistinct = incompatibleDistinct.value as Record<
    string,
    unknown
  >;
  assertEqual(
    rejectedDistinct.status,
    "unsupported",
    "incompatible SELECT DISTINCT proposal status",
  );
  assertEqual(
    rejectedDistinct.reason,
    "distinct_unsupported",
    "incompatible SELECT DISTINCT reason",
  );
  assertEqual(
    rejectedDistinct.base_sql,
    incompatibleDistinctSql,
    "incompatible SELECT DISTINCT source preservation",
  );
  assertEqual(
    rejectedDistinct.proposed_sql,
    undefined,
    "incompatible SELECT DISTINCT proposal omission",
  );

  const relationFreeContext = {
    relation: "",
    columns: [],
    variables: [],
    stable_key: [],
  };
  const scalarAnalysisResponse = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: { sql: "SELECT 1", context: relationFreeContext },
  });
  assertEqual(
    scalarAnalysisResponse.ok,
    true,
    "relation-free analysis protocol envelope",
  );
  const scalarAnalysis = scalarAnalysisResponse.value as Record<
    string,
    unknown
  >;
  assertEqual(
    scalarAnalysis.status,
    "supported",
    "relation-free scalar SELECT status",
  );
  const scalarCapabilities = scalarAnalysis.capabilities as Record<
    string,
    Record<string, unknown>
  >;
  assertEqual(
    scalarCapabilities.distinct.enabled,
    true,
    "relation-free DISTINCT capability",
  );
  assertEqual(
    scalarCapabilities.columns.enabled,
    false,
    "relation-free column capability",
  );
  const scalarProposalResponse = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql: "SELECT 1",
      context: relationFreeContext,
      operation: { kind: "set_distinct", distinct: true },
    },
  });
  const scalarProposal = scalarProposalResponse.value as Record<
    string,
    unknown
  >;
  assertEqual(
    scalarProposal.status,
    "supported",
    "relation-free DISTINCT proposal status",
  );
  assertEqual(
    scalarProposal.proposed_sql,
    "SELECT DISTINCT 1",
    "relation-free DISTINCT proposal SQL",
  );

  const unsupported = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: {
      sql: 'SELECT * FROM "Task" JOIN "Task" AS other ON true',
      context,
    },
  });
  const unsupportedValue = unsupported.value as Record<string, unknown>;
  assertEqual(unsupportedValue.status, "unsupported", "JOIN capability status");
  assertEqual(unsupportedValue.reason, "join_unsupported", "JOIN reason code");

  const joinSql = 'SELECT * FROM "Task" LIMIT 5';
  const joinProposal = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql: joinSql,
      context,
      operation: {
        kind: "set_join",
        left_column: "title",
        right_column: "title",
      },
    },
  });
  assertEqual(joinProposal.ok, true, "JOIN proposal protocol envelope");
  const joined = joinProposal.value as Record<string, unknown>;
  assertEqual(joined.status, "supported", "self-join proposal status");
  assertEqual(
    joined.proposed_sql,
    'SELECT * FROM "Task" AS "_ugoite_left" INNER JOIN "Task" AS "_ugoite_right" ON "_ugoite_left"."title" = "_ugoite_right"."title" LIMIT 5',
    "self-join SQL and preserved LIMIT",
  );
  const joinAnalysis = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: { sql: joined.proposed_sql, context },
  });
  const analyzedJoin = joinAnalysis.value as Record<string, unknown>;
  assertEqual(analyzedJoin.status, "supported", "self-join analysis status");
  assertEqual(
    JSON.stringify(analyzedJoin.joins),
    JSON.stringify([{
      left_alias: { name: "_ugoite_left", quoted: true },
      left_column: "title",
      relation: "Task",
      right_alias: { name: "_ugoite_right", quoted: true },
      right_column: "title",
    }]),
    "self-join descriptor",
  );

  const joinedClausesSql =
    'SELECT * FROM "Task" AS left_rows INNER JOIN "Task" AS right_rows ON left_rows.title = right_rows.title WHERE right_rows.title = $title ORDER BY left_rows._ugoite_id DESC LIMIT 8';
  const joinedClauses = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: { sql: joinedClausesSql, context },
  });
  const joinedClausesValue = joinedClauses.value as Record<string, unknown>;
  assertEqual(joinedClausesValue.status, "supported", "joined clauses status");
  assertEqual(
    JSON.stringify(joinedClausesValue.filters),
    JSON.stringify([{
      column: "title",
      operator: "eq",
      parameter: "title",
      qualifier: { name: "right_rows", quoted: false },
    }]),
    "qualified join filter descriptor",
  );
  assertEqual(
    JSON.stringify(joinedClausesValue.order_by),
    JSON.stringify([{
      column: "_ugoite_id",
      direction: "desc",
      qualifier: { name: "left_rows", quoted: false },
    }]),
    "qualified join ordering descriptor",
  );

  const clausesToQualifySql =
    'SELECT * FROM "Task" WHERE "title" = $title ORDER BY "_ugoite_id" DESC LIMIT 5';
  const clausesToQualify = await invokeWasm(instance.exports, {
    action: "domain.propose_sql_structure_operation",
    value: {
      sql: clausesToQualifySql,
      context,
      operation: {
        kind: "set_join",
        left_column: "title",
        right_column: "title",
      },
    },
  });
  const qualifiedProposal = clausesToQualify.value as Record<string, unknown>;
  assertEqual(
    qualifiedProposal.status,
    "supported",
    "adding join to existing clauses status",
  );
  assertEqual(
    qualifiedProposal.proposed_sql,
    'SELECT * FROM "Task" AS "_ugoite_left" INNER JOIN "Task" AS "_ugoite_right" ON "_ugoite_left"."title" = "_ugoite_right"."title" WHERE "_ugoite_left"."title" = $title ORDER BY "_ugoite_left"."_ugoite_id" DESC LIMIT 5',
    "adding join qualifies existing clauses and preserves their other source text",
  );

  const uppercaseIdentifiers = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: {
      sql:
        'SELECT * FROM "Task" AS LEFT_ROWS INNER JOIN "Task" AS RIGHT_ROWS ON LEFT_ROWS.TITLE = RIGHT_ROWS.TITLE WHERE RIGHT_ROWS.TITLE = $title ORDER BY LEFT_ROWS._UGOITE_ID DESC',
      context,
    },
  });
  assertEqual(
    (uppercaseIdentifiers.value as Record<string, unknown>).status,
    "supported",
    "unquoted uppercase join identifiers resolve using SQL case folding",
  );

  const unqualifiedJoinedClause = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: {
      sql:
        'SELECT * FROM "Task" AS left_rows INNER JOIN "Task" AS right_rows ON left_rows.title = right_rows.title WHERE title = $title',
      context,
    },
  });
  assertEqual(
    (unqualifiedJoinedClause.value as Record<string, unknown>).status,
    "unsupported",
    "ambiguous unqualified join filter fails closed",
  );

  const unsupportedOuterJoin = await invokeWasm(instance.exports, {
    action: "domain.analyze_sql_structure",
    value: {
      sql:
        'SELECT * FROM "Task" AS t LEFT JOIN "Task" AS other ON t.title = other.title',
      context,
    },
  });
  const unsupportedOuterValue = unsupportedOuterJoin.value as Record<
    string,
    unknown
  >;
  assertEqual(
    unsupportedOuterValue.status,
    "unsupported",
    "outer join remains outside the editor contract",
  );
  assertEqual(
    unsupportedOuterValue.reason,
    "join_unsupported",
    "outer join fail-closed reason",
  );
}

if (import.meta.main) {
  await main();
  console.log("SQL structure Rust/WASM parity fixtures passed");
}
