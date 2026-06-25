#!/usr/bin/env -S deno run --allow-read

const ROOT = new URL("../", import.meta.url);

const fail = (message: string): never => {
  throw new Error(message);
};

const readText = async (relativePath: string): Promise<string> =>
  await Deno.readTextFile(new URL(relativePath, ROOT));

const quotedItems = (block: string): string[] =>
  [...block.matchAll(/"([a-z][a-z0-9_.]+)"/g)].map((match) => match[1]);

const rustManifestOperations = (source: string): string[] => {
  const match = source.match(
    /pub const SUPPORTED_OPERATIONS: &\[&str\] = &\[(.*?)\];/s,
  );
  if (!match) fail("Rust SUPPORTED_OPERATIONS manifest was not found");
  return quotedItems(match[1]);
};

const typescriptManifestOperations = (source: string): string[] => {
  const match = source.match(
    /export const UGOITE_API_OPERATIONS = \[(.*?)\] as const;/s,
  );
  if (!match) fail("TypeScript UGOITE_API_OPERATIONS manifest was not found");
  return quotedItems(match[1]);
};

const frontendOperationUses = async (): Promise<Map<string, string[]>> => {
  const pattern = /protocolFetch(?:<.*?>)?\s*\(\s*["']([^"']+)/gs;
  const uses = new Map<string, string[]>();
  for await (const entry of Deno.readDir(new URL("frontend/src/lib/", ROOT))) {
    if (!entry.isFile || !entry.name.endsWith("-api.ts")) continue;
    const relativePath = `frontend/src/lib/${entry.name}`;
    const content = await readText(relativePath);
    for (const match of content.matchAll(pattern)) {
      const operation = match[1];
      uses.set(operation, [...(uses.get(operation) ?? []), relativePath]);
    }
  }
  return uses;
};

const cliOperationUses = async (): Promise<Map<string, string[]>> => {
  const pattern = /http::execute\s*\(\s*[^,]+,\s*["']([^"']+)/gs;
  const uses = new Map<string, string[]>();
  for await (
    const entry of Deno.readDir(new URL("crates/ugoite-cli/src/commands/", ROOT))
  ) {
    if (!entry.isFile || !entry.name.endsWith(".rs")) continue;
    const relativePath = `crates/ugoite-cli/src/commands/${entry.name}`;
    const content = await readText(relativePath);
    for (const match of content.matchAll(pattern)) {
      const operation = match[1];
      uses.set(operation, [...(uses.get(operation) ?? []), relativePath]);
    }
  }
  return uses;
};

const dependencyKeysFromManifest = async (
  relativePath: string,
): Promise<Set<string>> => {
  const dependencies = new Set<string>();
  let inDependencies = false;
  const content = await readText(relativePath);
  for (const rawLine of content.split("\n")) {
    const line = rawLine.trim();
    if (line.startsWith("[")) {
      inDependencies = line === "[dependencies]";
      continue;
    }
    if (!inDependencies || !line || line.startsWith("#") || !line.includes("=")) {
      continue;
    }
    const [key] = line.split("=", 1);
    if (key.trim()) dependencies.add(key.trim());
  }
  return dependencies;
};

const checkCargoManifests = async (): Promise<void> => {
  for await (const entry of walk(new URL("./", ROOT))) {
    if (!entry.isFile || !entry.path.endsWith("Cargo.toml")) continue;
    await Deno.readTextFile(entry.path);
  }
  await readText("Cargo.lock");

  const apiDependencies = await dependencyKeysFromManifest(
    "crates/ugoite-api-client/Cargo.toml",
  );
  const forbiddenApi = new Set([
    "axum",
    "opendal",
    "reqwest",
    "tokio",
    "ugoite-core",
    "ugoite-domain",
    "ugoite-storage",
    "wasm-bindgen",
    "web-sys",
  ]);
  const unexpectedApi = [...apiDependencies].filter((dep) => forbiddenApi.has(dep))
    .sort();
  if (unexpectedApi.length > 0) {
    fail(`ugoite-api-client has forbidden dependencies: ${unexpectedApi.join(", ")}`);
  }

  const wasmDependencies = await dependencyKeysFromManifest(
    "crates/ugoite-wasm/Cargo.toml",
  );
  const forbiddenWasm = new Set([
    "opendal",
    "reqwest",
    "tokio",
    "ugoite-core",
    "ugoite-storage",
  ]);
  const unexpectedWasm = [...wasmDependencies].filter((dep) =>
    forbiddenWasm.has(dep)
  ).sort();
  if (unexpectedWasm.length > 0) {
    fail(`ugoite-wasm has forbidden dependencies: ${unexpectedWasm.join(", ")}`);
  }
};

const checkSourceBoundaries = async (): Promise<void> => {
  for await (const entry of Deno.readDir(new URL("frontend/src/lib/", ROOT))) {
    if (!entry.isFile || !entry.name.endsWith("-api.ts")) continue;
    const relativePath = `frontend/src/lib/${entry.name}`;
    const content = await readText(relativePath);
    if (content.includes("apiFetch")) {
      fail(`${relativePath} imports or calls apiFetch directly`);
    }
  }

  const forbiddenTokens = [
    "http::http_get",
    "http::http_post",
    "http::http_put",
    "http::http_patch",
    "http::http_delete",
    'format!("{base}/',
  ];
  for await (
    const entry of Deno.readDir(new URL("crates/ugoite-cli/src/commands/", ROOT))
  ) {
    if (!entry.isFile || !entry.name.endsWith(".rs")) continue;
    const relativePath = `crates/ugoite-cli/src/commands/${entry.name}`;
    const content = await readText(relativePath);
    for (const token of forbiddenTokens) {
      if (content.includes(token)) {
        fail(`${relativePath} bypasses the portable operation boundary via ${token}`);
      }
    }
  }
};

const walk = async function* (dir: URL): AsyncGenerator<Deno.DirEntry & {
  path: string;
}> {
  for await (const entry of Deno.readDir(dir)) {
    const path = new URL(entry.name, dir);
    if (entry.isDirectory) {
      yield* walk(new URL(`${entry.name}/`, dir));
      continue;
    }
    yield { ...entry, path: path.pathname };
  }
};

const ensureUnique = (label: string, values: string[]) => {
  if (values.length !== new Set(values).size) {
    fail(`${label} operation manifest contains duplicates`);
  }
};

const sectionBetween = (
  source: string,
  start: string,
  end: string,
  label: string,
): string => {
  const startIndex = source.indexOf(start);
  if (startIndex === -1) fail(`${label} start marker was not found`);
  const rest = source.slice(startIndex + start.length);
  const endIndex = rest.indexOf(end);
  if (endIndex === -1) fail(`${label} end marker was not found`);
  return rest.slice(0, endIndex);
};

const main = async (): Promise<void> => {
  const rustSource = await readText("crates/ugoite-api-client/src/lib.rs");
  const tsSource = await readText("frontend/src/lib/ugoite-client/protocol.ts");
  const rustOperations = rustManifestOperations(rustSource);
  const tsOperations = typescriptManifestOperations(tsSource);

  ensureUnique("Rust", rustOperations);
  ensureUnique("TypeScript", tsOperations);

  if (
    rustOperations.length !== tsOperations.length ||
    rustOperations.some((operation, index) => operation !== tsOperations[index])
  ) {
    const rustSet = new Set(rustOperations);
    const tsSet = new Set(tsOperations);
    const rustOnly = [...rustSet].filter((item) => !tsSet.has(item)).sort();
    const tsOnly = [...tsSet].filter((item) => !rustSet.has(item)).sort();
    fail(
      `Rust/TypeScript operation manifests differ: rust_only=${JSON.stringify(rustOnly)}, ts_only=${JSON.stringify(tsOnly)}`,
    );
  }

  const frontendUses = await frontendOperationUses();
  const cliUses = await cliOperationUses();
  const supported = new Set(rustOperations);
  for (const [label, uses] of [["frontend", frontendUses], ["CLI", cliUses]] as const) {
    const unknown = [...uses.keys()].filter((operation) => !supported.has(operation))
      .sort();
    if (unknown.length > 0) {
      fail(`${label} uses unknown portable operations: ${unknown.join(", ")}`);
    }
  }

  const prepareSection = sectionBetween(
    rustSource,
    "pub fn prepare_request",
    "pub fn decode_response",
    "prepare_request section",
  );
  const decodeSection = sectionBetween(
    rustSource,
    "fn operation_spec",
    "fn required_string",
    "operation_spec section",
  );
  for (const operation of rustOperations) {
    const token = `"${operation}" =>`;
    if (!prepareSection.includes(token)) fail(`prepare_request is missing ${operation}`);
    if (!decodeSection.includes(token)) fail(`operation_spec is missing ${operation}`);
  }

  await checkCargoManifests();
  await checkSourceBoundaries();

  console.log(
    `portable API client structural validation passed: ${rustOperations.length} operations, ${frontendUses.size} used by frontend, ${cliUses.size} used by CLI`,
  );
};

if (import.meta.main) {
  try {
    await main();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`portable API client structural validation failed: ${message}`);
    Deno.exit(1);
  }
}
