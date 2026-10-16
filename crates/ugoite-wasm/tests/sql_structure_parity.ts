const workspaceRoot = new URL("../../..", import.meta.url).pathname;

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
  assertEqual(proposed.requires_confirmation, true, "explicit diff confirmation");
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
  assertEqual(sortedProposal.requires_confirmation, true, "sort diff confirmation");

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
}

if (import.meta.main) {
  await main();
  console.log("SQL structure Rust/WASM parity fixtures passed");
}
