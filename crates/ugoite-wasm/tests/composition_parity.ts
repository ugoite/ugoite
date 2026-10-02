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
    throw new Error(`${label}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

async function invokeWasm(
  exports: WebAssembly.Exports,
  request: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const memory = exports.memory as WebAssembly.Memory;
  const allocate = exports.ugoite_alloc as (length: number) => number;
  const deallocate = exports.ugoite_dealloc as (pointer: number, length: number) => void;
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
    await runCommand("cargo", ["metadata", "--no-deps", "--format-version", "1"]),
  ) as { target_directory: string };
  const wasmPath = `${metadata.target_directory}/wasm32-unknown-unknown/debug/ugoite_wasm.wasm`;
  const wasmBytes = await Deno.readFile(wasmPath);
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  const fixturePath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml",
    import.meta.url,
  );
  const canonicalPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/monthly-expense.canonical.ugcomp.yaml",
    import.meta.url,
  );
  const fingerprintPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/monthly-expense.fingerprint.txt",
    import.meta.url,
  );
  const yaml = await Deno.readTextFile(fixturePath);
  const canonicalYaml = await Deno.readTextFile(canonicalPath);
  const fingerprint = (await Deno.readTextFile(fingerprintPath)).trim();
  const response = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition",
    value: { yaml },
  });
  const responseValue = response.value as Record<string, unknown>;
  assertEqual(response.ok, true, "monthly-expense parse result");
  assertEqual(responseValue.canonical_yaml, canonicalYaml, "canonical YAML");
  assertEqual(responseValue.fingerprint, fingerprint, "semantic fingerprint");

  for (const [invalidYaml, expectedCode] of [
    ["format_version: 2\n", "unsupported_format_version"],
    ["format_version: 1\nname: [invalid\n", "invalid_composition"],
  ] as const) {
    const invalidResponse = await invokeWasm(instance.exports, {
      action: "domain.canonicalize_composition",
      value: { yaml: invalidYaml },
    });
    const error = invalidResponse.error as Record<string, unknown>;
    assertEqual(invalidResponse.ok, false, "invalid document result");
    assertEqual(error.kind, "composition_diagnostic", "diagnostic kind");
    assertEqual(error.code, expectedCode, "diagnostic code");
  }
}

if (import.meta.main) {
  await main();
  console.log("Composition native/WASM parity fixtures passed");
}
