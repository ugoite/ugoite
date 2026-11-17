import ts from "typescript";

const frontendConfig = "frontend/tsconfig.json";
const sourcePrefix = "frontend/src/";

export function isFrontendApplicationTypeScript(path: string): boolean {
  return path.startsWith(sourcePrefix) &&
    /\.tsx?$/.test(path) &&
    !/\.d\.ts$/.test(path) &&
    !/\.test\.tsx?$/.test(path);
}

export function changedSourceDiffRange(base: string): string {
  return `${base}..HEAD`;
}

async function gitOutput(args: string[]): Promise<string> {
  const result = await new Deno.Command("git", {
    args,
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!result.success) {
    throw new Error(new TextDecoder().decode(result.stderr));
  }
  return new TextDecoder().decode(result.stdout).trim();
}

async function resolveBase(): Promise<string | undefined> {
  const configuredBase = Deno.env.get("UGOITE_FRONTEND_TYPECHECK_BASE")?.trim();
  if (configuredBase) return configuredBase;

  const result = await new Deno.Command("git", {
    args: ["rev-parse", "--verify", "--quiet", "origin/main"],
    stdout: "null",
    stderr: "null",
  }).output();
  if (result.success) return "origin/main";
  return undefined;
}

async function main(): Promise<void> {
  const base = await resolveBase();
  if (!base) {
    console.log(
      "Changed frontend TypeScript check skipped: no base ref is available.",
    );
    return;
  }

  const committedPaths = await gitOutput([
    "diff",
    "--name-only",
    "--diff-filter=ACMR",
    // CI fetches base and PR merge commits shallowly, so avoid merge-base.
    changedSourceDiffRange(base),
    "--",
    sourcePrefix,
  ]);
  const stagedPaths = await gitOutput([
    "diff",
    "--cached",
    "--name-only",
    "--diff-filter=ACMR",
    "--",
    sourcePrefix,
  ]);
  const unstagedPaths = await gitOutput([
    "diff",
    "--name-only",
    "--diff-filter=ACMR",
    "--",
    sourcePrefix,
  ]);
  const untrackedPaths = await gitOutput([
    "ls-files",
    "--others",
    "--exclude-standard",
    "--",
    sourcePrefix,
  ]);
  const files = [
    ...new Set([
      ...committedPaths.split("\n"),
      ...stagedPaths.split("\n"),
      ...unstagedPaths.split("\n"),
      ...untrackedPaths.split("\n"),
    ]),
  ].filter(isFrontendApplicationTypeScript);
  if (files.length === 0) {
    console.log(
      "Changed frontend TypeScript check: no changed application source files.",
    );
    return;
  }

  const configPath = `${Deno.cwd()}/${frontendConfig}`;
  const config = ts.readConfigFile(configPath, ts.sys.readFile);
  if (config.error) {
    throw new Error(
      ts.flattenDiagnosticMessageText(config.error.messageText, "\n"),
    );
  }
  const frontendRoot = `${Deno.cwd()}/frontend`;
  const parsed = ts.parseJsonConfigFileContent(
    config.config,
    ts.sys,
    frontendRoot,
    undefined,
    configPath,
  );
  if (parsed.errors.length > 0) {
    console.error(ts.formatDiagnosticsWithColorAndContext(parsed.errors, {
      getCanonicalFileName: (path) => path,
      getCurrentDirectory: Deno.cwd,
      getNewLine: () => "\n",
    }));
    Deno.exit(1);
  }
  if (!parsed.options.strict) {
    throw new Error(`${frontendConfig} must keep strict mode enabled.`);
  }

  const rootNames = files.map((file) => `${Deno.cwd()}/${file}`);
  const program = ts.createProgram({
    rootNames,
    options: {
      ...parsed.options,
      allowImportingTsExtensions: true,
      // Check application imports while avoiding diagnostics inside vendor declarations.
      skipLibCheck: true,
    },
  });
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length > 0) {
    console.error(ts.formatDiagnosticsWithColorAndContext(diagnostics, {
      getCanonicalFileName: (path) => path,
      getCurrentDirectory: Deno.cwd,
      getNewLine: () => "\n",
    }));
    Deno.exit(1);
  }

  const checkedSourceCount =
    program.getSourceFiles().filter((source) =>
      source.fileName.startsWith(`${frontendRoot}/src/`)
    ).length;
  console.log(
    `Changed frontend TypeScript check passed: ${files.length} changed roots; ${checkedSourceCount} application sources in the import graph.`,
  );
}

if (import.meta.main) await main();
