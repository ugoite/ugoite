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

function assertJsonEqual(actual: unknown, expected: unknown, label: string): void {
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
    "../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml",
    import.meta.url,
  );
  const labeledFixturePath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/monthly-expense-labeled.ugcomp.yaml",
    import.meta.url,
  );
  const metricValueFieldsFixturePath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/metric-value-fields.ugcomp.yaml",
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
  const labeledCanonicalPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/monthly-expense-labeled.canonical.ugcomp.yaml",
    import.meta.url,
  );
  const labeledFingerprintPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/monthly-expense-labeled.fingerprint.txt",
    import.meta.url,
  );
  const metricValueFieldsCanonicalPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/metric-value-fields.canonical.ugcomp.yaml",
    import.meta.url,
  );
  const metricValueFieldsFingerprintPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/metric-value-fields.fingerprint.txt",
    import.meta.url,
  );
  const invalidListItemPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/unknown-list-item-field.ugcomp.yaml",
    import.meta.url,
  );
  const unknownFormatVersionPath = new URL(
    "../../../e2e/fixtures/composition/unknown-format-version.ugcomp.yaml",
    import.meta.url,
  );
  const metricPageEvaluationPath = new URL(
    "../../ugoite-domain/tests/fixtures/composition/metric-page-evaluation.json",
    import.meta.url,
  );
  const yaml = await Deno.readTextFile(fixturePath);
  const labeledYaml = await Deno.readTextFile(labeledFixturePath);
  const metricValueFieldsYaml = await Deno.readTextFile(metricValueFieldsFixturePath);
  const canonicalYaml = await Deno.readTextFile(canonicalPath);
  const fingerprint = (await Deno.readTextFile(fingerprintPath)).trim();
  const labeledCanonicalYaml = await Deno.readTextFile(labeledCanonicalPath);
  const labeledFingerprint = (await Deno.readTextFile(labeledFingerprintPath)).trim();
  const metricValueFieldsCanonicalYaml = await Deno.readTextFile(
    metricValueFieldsCanonicalPath,
  );
  const metricValueFieldsFingerprint = (
    await Deno.readTextFile(metricValueFieldsFingerprintPath)
  ).trim();
  const invalidListItemYaml = await Deno.readTextFile(invalidListItemPath);
  const unknownFormatVersionYaml = await Deno.readTextFile(unknownFormatVersionPath);
  const metricPageEvaluationFixtures = JSON.parse(
    await Deno.readTextFile(metricPageEvaluationPath),
  ) as {
    name: string;
    value: Record<string, unknown>;
    expected_response: Record<string, unknown>;
  }[];
  const unreferencedComponentYaml = yaml.replace(
    "      components: [transactions]",
    "      components: []",
  );
  if (unreferencedComponentYaml === yaml) {
    throw new Error("Could not build the unreferenced-component fixture");
  }
  const response = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition",
    value: { yaml },
  });
  const responseValue = response.value as Record<string, unknown>;
  assertEqual(response.ok, true, "monthly-expense parse result");
  assertEqual(responseValue.canonical_yaml, canonicalYaml, "canonical YAML");
  assertEqual(responseValue.fingerprint, fingerprint, "semantic fingerprint");

  const typedDocument = structuredClone(
    responseValue.document as Record<string, unknown>,
  );
  const unnormalizedTypedDocument = structuredClone(typedDocument);
  const typedSpec = unnormalizedTypedDocument.spec as Record<string, unknown>;
  const typedSources = typedSpec.sources as Record<string, unknown>[];
  const entryQuerySource = typedSources.find((source) =>
    source.kind === "entry_query"
  );
  if (!entryQuerySource || !Array.isArray(entryQuerySource.field_schema)) {
    throw new Error("Typed fixture must contain an EntryQuery field schema");
  }
  const fieldSchema = entryQuerySource.field_schema;
  if (fieldSchema.length < 2) {
    throw new Error("Typed fixture field schema must exercise ordering");
  }
  const normalizedFieldSchema = JSON.stringify(normalizeJson(fieldSchema));
  fieldSchema.reverse();
  if (JSON.stringify(normalizeJson(fieldSchema)) === normalizedFieldSchema) {
    throw new Error("Could not make the typed field schema non-normalized");
  }

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
    typedDocument,
    "typed normalized document",
  );
  assertEqual(
    typedCanonicalValue.canonical_yaml,
    canonicalYaml,
    "typed canonical YAML",
  );
  assertEqual(
    typedCanonicalValue.fingerprint,
    fingerprint,
    "typed semantic fingerprint",
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
  assertEqual(futureVersionTypedResponse.ok, false, "future typed document result");
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
  assertEqual(unknownV1FieldResponse.ok, false, "unknown v1 typed document result");
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

  const labeledResponse = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition",
    value: { yaml: labeledYaml },
  });
  const labeledResponseValue = labeledResponse.value as Record<string, unknown>;
  assertEqual(labeledResponse.ok, true, "labeled monthly-expense parse result");
  assertEqual(
    labeledResponseValue.canonical_yaml,
    labeledCanonicalYaml,
    "labeled canonical YAML",
  );
  assertEqual(
    labeledResponseValue.fingerprint,
    labeledFingerprint,
    "labeled semantic fingerprint",
  );

  const metricValueFieldsResponse = await invokeWasm(instance.exports, {
    action: "domain.canonicalize_composition",
    value: { yaml: metricValueFieldsYaml },
  });
  const metricValueFieldsResponseValue = metricValueFieldsResponse.value as Record<
    string,
    unknown
  >;
  assertEqual(
    metricValueFieldsResponse.ok,
    true,
    "typed metric value fields parse result",
  );
  assertEqual(
    metricValueFieldsResponseValue.canonical_yaml,
    metricValueFieldsCanonicalYaml,
    "typed metric value fields canonical YAML",
  );
  assertEqual(
    metricValueFieldsResponseValue.fingerprint,
    metricValueFieldsFingerprint,
    "typed metric value fields semantic fingerprint",
  );

  const invalidMetricValueFieldYaml = yaml.replace(
    "kind: sql_column",
    "kind: unknown_column",
  );
  if (invalidMetricValueFieldYaml === yaml) {
    throw new Error("Could not build an unknown metric value-field variant fixture");
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
  assertEqual(unsupportedVersionResponse.ok, false, "unsupported version result");
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
      [invalidListItemYaml, "invalid_composition"],
      [invalidMetricValueFieldYaml, "invalid_composition"],
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

  for (const fixture of metricPageEvaluationFixtures) {
    const metricResponse = await invokeWasm(instance.exports, {
      action: "domain.evaluate_composition_metric_page",
      value: fixture.value,
    });
    assertEqual(
      JSON.stringify(normalizeJson(metricResponse)),
      JSON.stringify(normalizeJson(fixture.expected_response)),
      `metric page evaluation: ${fixture.name}`,
    );
  }

  const oversizedMetricPageResponse = await invokeWasm(instance.exports, {
    action: "domain.evaluate_composition_metric_page",
    value: {
      expected_result_type: "integer",
      is_complete: true,
      row_count: 4_294_967_296,
      selected_column_count: 1,
    },
  });
  const oversizedMetricPageError = oversizedMetricPageResponse.error as Record<
    string,
    unknown
  >;
  assertEqual(oversizedMetricPageResponse.ok, false, "oversized metric page result");
  assertEqual(
    oversizedMetricPageError.kind,
    "domain_validation",
    "oversized metric page error kind",
  );
  assertEqual(
    oversizedMetricPageError.message,
    "value.row_count must be a non-negative integer",
    "oversized metric page count is target-independent",
  );
}

if (import.meta.main) {
  await main();
  console.log("Composition native/WASM parity fixtures passed");
}
