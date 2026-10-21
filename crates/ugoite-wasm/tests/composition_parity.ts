const workspaceRoot = new URL("../../..", import.meta.url).pathname;

const MAX_COMPOSITION_YAML_BYTES = 64 * 1024;
const MAX_COMPOSITION_YAML_DEPTH = 64;
const MAX_COMPOSITION_COLLECTION_ITEMS = 256;

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

function assertJsonEqual(
  actual: unknown,
  expected: unknown,
  label: string,
): void {
  assertEqual(
    JSON.stringify(normalizeJson(actual)),
    JSON.stringify(normalizeJson(expected)),
    label,
  );
}

function normalizeJson(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeJson);
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value).sort(([left], [right]) =>
      left < right ? -1 : left > right ? 1 : 0
    );
    return Object.fromEntries(
      entries.map(([key, entry]) => [key, normalizeJson(entry)]),
    );
  }
  return value;
}

function compositionYamlResponse(yaml: string): Record<string, unknown> {
  return {
    action: "domain.canonicalize_composition",
    value: { yaml },
  };
}

function paddedCompositionYaml(yaml: string, targetBytes: number): string {
  const currentBytes = new TextEncoder().encode(yaml).length;
  if (!yaml.endsWith("\n") || currentBytes >= targetBytes) {
    throw new Error(
      "Cannot construct the requested Composition byte boundary",
    );
  }
  return `${yaml}#${"x".repeat(targetBytes - currentBytes - 1)}`;
}

function compositionVariablesYaml(yaml: string, targetCount: number): string {
  const variablesSection = yaml.split("  variables:\n")[1]?.split(
    "  components:\n",
  )[0];
  if (!variablesSection) {
    throw new Error("Could not find the Composition variables");
  }
  const existingCount = (variablesSection.match(/^    - id:/gm) ?? []).length;
  if (targetCount < existingCount) {
    throw new Error("Cannot reduce the Composition variable count");
  }
  const constants = Array.from(
    { length: targetCount - existingCount },
    (_, index) => {
      const id = `extra_${existingCount + index}`;
      return `    - id: ${id}\n      kind: constant\n      value_type:\n        scalar_type: integer\n        nullable: true\n      value:\n        kind: "null"`;
    },
  ).join("\n");
  if (!yaml.includes("  components:\n")) {
    throw new Error("Could not locate the Composition component collection");
  }
  return yaml.replace("  components:\n", `${constants}\n  components:\n`);
}

function nestedFutureCompositionYaml(depth: number): string {
  return `format: ugoite.composition\nformat_version: 2\nvalue: ${
    "[".repeat(depth)
  }0${"]".repeat(depth)}\n`;
}

function assertCompositionDiagnostic(
  response: Record<string, unknown>,
  expectedCode: string,
  label: string,
): void {
  assertEqual(response.ok, false, `${label} result`);
  const error = response.error as Record<string, unknown>;
  assertEqual(error.kind, "composition_diagnostic", `${label} diagnostic kind`);
  assertEqual(error.code, expectedCode, `${label} diagnostic code`);
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
  const fixturePath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/sql-first.ugcomp.yaml",
    import.meta.url,
  );
  const canonicalPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/sql-first.canonical.ugcomp.yaml",
    import.meta.url,
  );
  const fingerprintPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/sql-first.fingerprint.txt",
    import.meta.url,
  );
  const yaml = await Deno.readTextFile(fixturePath);
  const canonicalYaml = await Deno.readTextFile(canonicalPath);
  const fingerprint = (await Deno.readTextFile(fingerprintPath)).trim();
  const unknownFormatVersionYaml =
    "format: ugoite.composition\nformat_version: 2\nname: Future\nkind: dashboard\nspec: {}\n";
  const unreferencedComponentYaml = yaml.replace(
    "          - kind: component\n            component: transactions\n",
    "",
  );
  if (unreferencedComponentYaml === yaml) {
    throw new Error("Could not build the unreferenced-component fixture");
  }
  const response = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition",
    value: { yaml },
  });
  const responseValue = response.value as Record<string, unknown>;
  assertEqual(response.ok, true, "SQL-first Composition parse result");
  assertEqual(responseValue.canonical_yaml, canonicalYaml, "canonical YAML");
  assertEqual(responseValue.fingerprint, fingerprint, "semantic fingerprint");

  const atByteLimit = await invokeWasm(
    instance.exports,
    compositionYamlResponse(
      paddedCompositionYaml(yaml, MAX_COMPOSITION_YAML_BYTES),
    ),
  );
  assertEqual(atByteLimit.ok, true, "WASM parser accepts the byte limit");
  const overByteLimit = await invokeWasm(
    instance.exports,
    compositionYamlResponse(
      paddedCompositionYaml(yaml, MAX_COMPOSITION_YAML_BYTES + 1),
    ),
  );
  assertCompositionDiagnostic(
    overByteLimit,
    "invalid_composition",
    "WASM parser rejects input over the byte limit",
  );

  const collectionBoundaryYaml = compositionVariablesYaml(
    yaml,
    MAX_COMPOSITION_COLLECTION_ITEMS,
  );
  const atCollectionLimit = await invokeWasm(
    instance.exports,
    compositionYamlResponse(
      collectionBoundaryYaml,
    ),
  );
  assertEqual(
    atCollectionLimit.ok,
    true,
    "WASM parser accepts the collection item limit",
  );
  const overCollectionLimit = await invokeWasm(
    instance.exports,
    compositionYamlResponse(
      compositionVariablesYaml(yaml, MAX_COMPOSITION_COLLECTION_ITEMS + 1),
    ),
  );
  assertCompositionDiagnostic(
    overCollectionLimit,
    "invalid_composition",
    "WASM parser rejects input over the collection item limit",
  );

  const atDepthLimit = await invokeWasm(
    instance.exports,
    compositionYamlResponse(
      nestedFutureCompositionYaml(MAX_COMPOSITION_YAML_DEPTH - 1),
    ),
  );
  assertCompositionDiagnostic(
    atDepthLimit,
    "unsupported_format_version",
    "WASM parser accepts the nesting limit before version dispatch",
  );
  const overDepthLimit = await invokeWasm(
    instance.exports,
    compositionYamlResponse(
      nestedFutureCompositionYaml(MAX_COMPOSITION_YAML_DEPTH),
    ),
  );
  assertCompositionDiagnostic(
    overDepthLimit,
    "invalid_composition",
    "WASM parser rejects input over the nesting limit",
  );

  const typedDocument = structuredClone(
    responseValue.document as Record<string, unknown>,
  );
  const unnormalizedTypedDocument = structuredClone(typedDocument);
  const typedSpec = unnormalizedTypedDocument.spec as Record<string, unknown>;
  const typedVariables = typedSpec.variables as Record<string, unknown>[];
  const inlineSqlVariable = typedVariables.find((variable) =>
    variable.id === "expense_rows"
  );
  const inlineQuery = inlineSqlVariable?.query as Record<string, unknown>;
  if (!inlineSqlVariable || !Array.isArray(inlineQuery.form_bindings)) {
    throw new Error("Typed fixture must contain query-level Form bindings");
  }
  const formBindings = inlineQuery.form_bindings as Record<
    string,
    unknown
  >[];
  const secondBinding = { ...formBindings[0], relation: "Other" };
  formBindings.push(secondBinding);
  formBindings.reverse();
  const expectedTypedDocument = structuredClone(typedDocument);
  const expectedTypedSpec = expectedTypedDocument.spec as Record<
    string,
    unknown
  >;
  const expectedTypedVariables = expectedTypedSpec.variables as Record<
    string,
    unknown
  >[];
  const expectedInlineSqlVariable = expectedTypedVariables.find((variable) =>
    variable.id === "expense_rows"
  );
  const expectedInlineQuery = expectedInlineSqlVariable?.query as Record<
    string,
    unknown
  >;
  if (
    !expectedInlineSqlVariable ||
    !Array.isArray(expectedInlineQuery.form_bindings)
  ) {
    throw new Error(
      "Normalized fixture must contain query-level Form bindings",
    );
  }
  const expectedBindings = expectedInlineQuery.form_bindings as Record<
    string,
    unknown
  >[];
  expectedBindings.push({ ...expectedBindings[0], relation: "Other" });
  expectedBindings.sort((left, right) =>
    String(left.relation) < String(right.relation)
      ? -1
      : String(left.relation) > String(right.relation)
      ? 1
      : 0
  );

  const typedCanonicalResponse = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition_document",
    value: { document: unnormalizedTypedDocument },
  });
  const typedCanonicalValue = typedCanonicalResponse.value as Record<
    string,
    unknown
  >;
  assertEqual(typedCanonicalResponse.ok, true, "typed normalization result");
  assertJsonEqual(
    typedCanonicalValue.document,
    expectedTypedDocument,
    "typed normalized document",
  );
  if (typeof typedCanonicalValue.canonical_yaml !== "string") {
    throw new Error("typed canonical YAML must be a string");
  }
  if (
    typeof typedCanonicalValue.fingerprint !== "string" ||
    !/^[0-9a-f]{64}$/.test(typedCanonicalValue.fingerprint)
  ) {
    throw new Error("typed semantic fingerprint must be lowercase SHA-256");
  }
  const typedIdempotenceResponse = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition_document",
    value: { document: typedCanonicalValue.document },
  });
  const typedIdempotenceValue = typedIdempotenceResponse.value as Record<
    string,
    unknown
  >;
  assertEqual(
    typedIdempotenceResponse.ok,
    true,
    "typed normalization idempotence",
  );
  assertEqual(
    typedIdempotenceValue.canonical_yaml,
    typedCanonicalValue.canonical_yaml,
    "typed canonical YAML idempotence",
  );
  assertEqual(
    typedIdempotenceValue.fingerprint,
    typedCanonicalValue.fingerprint,
    "typed fingerprint idempotence",
  );

  const futureVersionDocument = structuredClone(typedDocument);
  futureVersionDocument.format_version = 2;
  futureVersionDocument.future_field = { enabled: true };
  const futureVersionTypedResponse = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition_document",
    value: { document: futureVersionDocument },
  });
  const futureVersionTypedError = futureVersionTypedResponse.error as Record<
    string,
    unknown
  >;
  assertEqual(
    futureVersionTypedResponse.ok,
    false,
    "future typed document result",
  );
  assertEqual(
    futureVersionTypedError.kind,
    "composition_diagnostic",
    "future typed document diagnostic kind",
  );
  assertEqual(
    futureVersionTypedError.code,
    "unsupported_format_version",
    "future typed document diagnostic code",
  );

  const unknownV1FieldDocument = structuredClone(typedDocument);
  unknownV1FieldDocument.future_field = { enabled: true };
  const unknownV1FieldResponse = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition_document",
    value: { document: unknownV1FieldDocument },
  });
  const unknownV1FieldError = unknownV1FieldResponse.error as Record<
    string,
    unknown
  >;
  assertEqual(
    unknownV1FieldResponse.ok,
    false,
    "unknown v1 typed document result",
  );
  assertEqual(
    unknownV1FieldError.kind,
    "composition_diagnostic",
    "unknown v1 typed document diagnostic kind",
  );
  assertEqual(
    unknownV1FieldError.code,
    "invalid_composition",
    "unknown v1 typed document diagnostic code",
  );

  const invalidVariableReferenceYaml = yaml.replace(
    "      variable: month_total",
    "      variable: missing",
  );
  if (invalidVariableReferenceYaml === yaml) {
    throw new Error(
      "Could not build an invalid graph variable reference fixture",
    );
  }
  const unsupportedOldSourceYaml = yaml.replace(
    "kind: inline_sql",
    "kind: entry_query",
  );
  if (unsupportedOldSourceYaml === yaml) {
    throw new Error("Could not build the unsupported old query-kind fixture");
  }

  // Keep version dispatch separate from complete v1 documents that fail
  // strict schema or layout validation below.
  const unsupportedVersionResponse = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition",
    value: { yaml: unknownFormatVersionYaml },
  });
  const unsupportedVersionError = unsupportedVersionResponse.error as Record<
    string,
    unknown
  >;
  assertEqual(
    unsupportedVersionResponse.ok,
    false,
    "unsupported version result",
  );
  assertEqual(
    unsupportedVersionError.kind,
    "composition_diagnostic",
    "unsupported version diagnostic kind",
  );
  assertEqual(
    unsupportedVersionError.code,
    "unsupported_format_version",
    "unsupported version diagnostic code",
  );

  // These inputs all carry the complete supported envelope, so they exercise
  // v1 validation diagnostics rather than YAML or version-probe failures.
  for (
    const [invalidYaml, expectedCode] of [
      [unsupportedOldSourceYaml, "invalid_composition"],
      [invalidVariableReferenceYaml, "invalid_composition"],
      [unreferencedComponentYaml, "invalid_composition"],
    ] as const
  ) {
    const invalidResponse = await invokeWasm(instance.exports, {
      action: "domain.canonicalize_composition",
      value: { yaml: invalidYaml },
    });
    const error = invalidResponse.error as Record<string, unknown>;
    assertEqual(invalidResponse.ok, false, "invalid document result");
    assertEqual(error.kind, "composition_diagnostic", "diagnostic kind");
    assertEqual(error.code, expectedCode, "diagnostic code");
  }

  const variableInputCases = [
    { scalar_type: "string", raw: "", expected: { status: "unset" } },
    {
      scalar_type: "string",
      raw: "日本語",
      expected: { status: "valid", value: "日本語" },
    },
    {
      scalar_type: "string",
      raw: "\u0000",
      expected: { status: "invalid" },
    },
    {
      scalar_type: "boolean",
      raw: "true",
      expected: { status: "valid", value: true },
    },
    { scalar_type: "boolean", raw: "TRUE", expected: { status: "invalid" } },
    {
      scalar_type: "integer",
      raw: "+9007199254740991",
      expected: { status: "valid", value: 9_007_199_254_740_991 },
    },
    { scalar_type: "integer", raw: "1.0", expected: { status: "invalid" } },
    {
      scalar_type: "integer",
      raw: "9007199254740992",
      expected: { status: "invalid" },
    },
    { scalar_type: "float", raw: "1", expected: { status: "valid", value: 1 } },
    { scalar_type: "float", raw: "1e309", expected: { status: "invalid" } },
    {
      scalar_type: "date",
      raw: "2024-02-29",
      expected: { status: "valid", value: "2024-02-29" },
    },
    { scalar_type: "date", raw: "2025-02-29", expected: { status: "invalid" } },
    {
      scalar_type: "timestamp",
      raw: "2026-10-13T23:59:59.123456789+09:00",
      expected: {
        status: "valid",
        value: "2026-10-13T23:59:59.123456789+09:00",
      },
    },
    {
      scalar_type: "timestamp",
      raw: "2026-10-13T14:59:59",
      expected: { status: "invalid" },
    },
    {
      scalar_type: "decimal",
      raw: "123.4500",
      expected: { status: "valid", value: "123.4500" },
    },
    {
      scalar_type: "json",
      raw: '{"x":1}',
      expected: { status: "valid", value: { x: 1 } },
    },
  ] as const;
  for (const { scalar_type, raw, expected } of variableInputCases) {
    const response = await invokeWasm(instance.exports, {
      action: "domain.parse_variable_graph_input",
      value: { value_type: { scalar_type, nullable: false }, raw },
    });
    assertEqual(response.ok, true, `${scalar_type} Input parser envelope`);
    assertJsonEqual(
      response.value,
      expected,
      `${scalar_type} Input parser result for ${JSON.stringify(raw)}`,
    );
  }

  const malformedVariableInput = await invokeWasm(instance.exports, {
    action: "domain.parse_variable_graph_input",
    value: {
      value_type: { scalar_type: "integer", nullable: false },
      raw: 1,
    },
  });
  const malformedVariableInputError = malformedVariableInput.error as Record<
    string,
    unknown
  >;
  assertEqual(
    malformedVariableInput.ok,
    false,
    "malformed Input parser result",
  );
  assertEqual(
    malformedVariableInputError.kind,
    "domain_validation",
    "malformed Input parser error kind",
  );
}

if (import.meta.main) {
  await main();
  console.log("Composition native/WASM parity fixtures passed");
}
