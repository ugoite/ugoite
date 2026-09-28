type SeedInput = {
  slug: string;
  seed: number;
  count: number;
  owner: string | null;
  profilePath: string;
  resourcePath: string;
};

type ExportInput = {
  pageSize: number;
  outputPath: string;
  summaryPath: string;
  resourcePath: string;
};

type TimedStepInput = { name: string; resourcePath: string };

const args = [...Deno.args];
const SHA = /^[0-9a-f]{40}$/i;

if (import.meta.main) await main();

async function main(): Promise<void> {
  const mode = args.shift();
  if (mode !== "query" && mode !== "export") {
    throw new Error(
      "usage: cp1_profile.ts <query|export> --output PATH --root PATH --started-ms MS --exit-code CODE",
    );
  }

  const options = new Map<string, string>();
  const seeds: SeedInput[] = [];
  const exports: ExportInput[] = [];
  const timedSteps: TimedStepInput[] = [];
  while (args.length > 0) {
    const key = args.shift();
    if (key === "--seed") {
      const [slug, seedText, countText, ownerText, profilePath, resourcePath] =
        args.splice(0, 6);
      if (!slug || !seedText || !countText || !profilePath || !resourcePath) {
        throw new Error(
          "--seed requires slug, seed, count, owner, profile path, and resource path",
        );
      }
      seeds.push({
        slug,
        seed: Number(seedText),
        count: Number(countText),
        owner: ownerText === "-" ? null : ownerText,
        profilePath,
        resourcePath,
      });
    } else if (key === "--export-run") {
      const [pageSizeText, outputPath, summaryPath, resourcePath] = args.splice(
        0,
        4,
      );
      if (!pageSizeText || !outputPath || !summaryPath || !resourcePath) {
        throw new Error(
          "--export-run requires page size, output path, summary path, and resource path",
        );
      }
      exports.push({
        pageSize: Number(pageSizeText),
        outputPath,
        summaryPath,
        resourcePath,
      });
    } else if (key === "--timed-step") {
      const [name, resourcePath] = args.splice(0, 2);
      if (!name || !resourcePath) {
        throw new Error("--timed-step requires a name and resource path");
      }
      timedSteps.push({ name, resourcePath });
    } else if (key?.startsWith("--")) {
      const value = args.shift();
      if (value === undefined) throw new Error(`${key} requires a value`);
      options.set(key.slice(2), value);
    } else {
      throw new Error(`unknown argument: ${key}`);
    }
  }

  const output = required(options, "output");
  const root = required(options, "root");
  const startedMs = finiteNumber(options.get("started-ms"), "started-ms");
  const exitCode = finiteNumber(options.get("exit-code"), "exit-code");
  const evidencePath = options.get("evidence-report");
  const now = Date.now();
  const seedProfiles = await Promise.all(seeds.map(async (seed) => {
    const generator = await readJsonOrNull(seed.profilePath);
    const profileComplete = isCompleteSampleProfile(generator);
    const process = await parseTimeResourceFile(seed.resourcePath);
    const generatorWallMicros = profileComplete
      ? (generator as Record<string, unknown>).total_wall_micros as number
      : null;
    const processWallMicros = process.elapsed_wall_seconds === null
      ? null
      : Math.round(process.elapsed_wall_seconds * 1_000_000);
    return {
      expected: {
        slug: seed.slug,
        seed: seed.seed,
        entry_count: seed.count,
        owner_display_name: seed.owner,
      },
      process_attempted: await fileExists(seed.profilePath) ||
        await fileExists(seed.resourcePath),
      generator,
      profile_complete: profileComplete,
      mutation_batch_summary: summarizeMutationBatches(generator),
      process,
      generator_wall_micros: generatorWallMicros,
      process_wall_micros: processWallMicros,
      process_minus_generator_wall_micros:
        processWallMicros === null || generatorWallMicros === null
          ? null
          : processWallMicros - generatorWallMicros,
      filesystem: await fileMetrics(await findSpacePath(root, seed.slug)),
    };
  }));

  const exportRuns = await Promise.all(exports.map(async (run) => ({
    page_size: run.pageSize,
    output: {
      rows: await countLines(run.outputPath),
      bytes: await fileSize(run.outputPath),
    },
    summary: await readJsonOrNull(run.summaryPath),
    process: await parseTimeResourceFile(run.resourcePath),
  })));
  const processSteps = await Promise.all(timedSteps.map(async (step) => ({
    name: step.name,
    process: await parseTimeResourceFile(step.resourcePath),
  })));
  const cliSource = mode === "export"
    ? Deno.env.get("UGOITE_SQL_EXPORT_CLI_BINARY")?.trim()
      ? "verified-artifact"
      : "local-build"
    : null;
  const cliTransferReportPath = mode === "export"
    ? Deno.env.get("UGOITE_SQL_EXPORT_CLI_TRANSFER_REPORT")?.trim() || null
    : null;
  const cliArtifactTransfer = cliTransferReportPath
    ? await readJsonOrNull(cliTransferReportPath)
    : null;
  if (
    exitCode === 0 && cliSource === "verified-artifact" &&
    cliTransferReportPath && cliArtifactTransfer === null
  ) {
    throw new Error(
      "successful SQL export is missing its CLI artifact transfer report",
    );
  }
  const notMeasured = [
    "cold Cargo compile time separated from cargo run and seed runtime",
    "Cargo incremental rebuild count and sccache hit/miss when unavailable",
    "per-Form read/convert, normalization/signing, batch validation, audit, and publication subspans inside the production mutation API",
    "storage read/write byte counts",
    "GitHub lane queue time, runner-minutes, and ci-required wall time",
  ];
  if (cliSource === "verified-artifact") {
    notMeasured.push(
      "CLI artifact archive compression CPU and upload duration; runtime image transfer and Docker load",
    );
  } else {
    notMeasured.push(
      "artifact archive size, compression CPU, upload/download duration, and extraction duration",
    );
  }
  const stageMicros = seedProfiles.flatMap((fixture) => {
    if (!isCompleteSampleProfile(fixture.generator)) return [];
    const profile = fixture.generator as Record<string, unknown>;
    return [
      profile.space_creation_micros,
      profile.owner_initialization_micros,
      profile.form_upsert_micros,
      profile.markdown_render_micros,
      profile.draft_conversion_micros,
      ...(Array.isArray(profile.mutation_batch_micros)
        ? profile.mutation_batch_micros
        : []),
    ].filter((value): value is number =>
      typeof value === "number" && Number.isFinite(value)
    );
  });
  const profilesComplete = seedProfiles.every((fixture) =>
    isCompleteSampleProfile(fixture.generator)
  );
  const knownStageMicros = profilesComplete
    ? stageMicros.reduce((total, value) => total + value, 0)
    : null;
  const generatorMicros = profilesComplete
    ? seedProfiles.reduce((total, fixture) => {
      const value =
        (fixture.generator as Record<string, unknown>).total_wall_micros;
      return total +
        (typeof value === "number" && Number.isFinite(value) ? value : 0);
    }, 0)
    : null;
  const seedProcessWallValues = seedProfiles.map((fixture) =>
    fixture.process_wall_micros
  );
  const processWallMicros = sumMeasurements(seedProcessWallValues);
  const allProcessWallMicros = sumMeasurements([
    ...seedProcessWallValues,
    ...exportRuns.map((run) =>
      run.process.elapsed_wall_seconds === null
        ? null
        : Math.round(run.process.elapsed_wall_seconds * 1_000_000)
    ),
    ...processSteps.map((step) =>
      step.process.elapsed_wall_seconds === null
        ? null
        : Math.round(step.process.elapsed_wall_seconds * 1_000_000)
    ),
  ]);
  const scriptWallMicros = Math.max(0, now - startedMs) * 1_000;
  const scriptWallReconciliation = reconcileWallDurations(
    scriptWallMicros,
    allProcessWallMicros,
  );
  const report = {
    schema_version: 1,
    measurement: mode === "query" ? "cp1-query" : "cp1-export",
    generated_at: new Date(now).toISOString(),
    source_sha: await gitHead(),
    checkout_dirty: await checkoutDirty(),
    ci_run_id: Deno.env.get("GITHUB_RUN_ID") ?? null,
    result: { exit_code: exitCode, successful: exitCode === 0 },
    wall: { elapsed_millis: Math.round(scriptWallMicros / 1_000) },
    environment: {
      os: Deno.build.os,
      arch: Deno.build.arch,
      deno_version: Deno.version.deno,
      cargo_profile: Deno.env.get("CARGO_PROFILE") ?? "dev",
    },
    seed_process_invocations:
      seedProfiles.filter((fixture) => fixture.process_attempted).length,
    fixtures: seedProfiles,
    export_runs: exportRuns,
    process_steps: processSteps,
    cli: mode === "export"
      ? {
        source: cliSource,
        source_sha: Deno.env.get("UGOITE_SQL_EXPORT_CLI_SOURCE_SHA") ?? null,
        artifact_transfer: cliArtifactTransfer,
      }
      : null,
    acceptance_evidence: evidencePath
      ? await readJsonOrNull(evidencePath)
      : null,
    measured_stages: {
      seed_process_wall_micros: processWallMicros,
      all_measured_process_wall_micros: allProcessWallMicros,
      ...scriptWallReconciliation,
      seed_generator_wall_micros: generatorMicros,
      process_minus_generator_wall_micros:
        processWallMicros === null || generatorMicros === null
          ? null
          : processWallMicros - generatorMicros,
      measured_stage_total_micros: knownStageMicros,
      unaccounted_seed_time_micros: generatorMicros !== null &&
          knownStageMicros !== null && generatorMicros >= knownStageMicros
        ? generatorMicros - knownStageMicros
        : null,
    },
    not_measured: notMeasured,
  };

  if (!SHA.test(report.source_sha)) {
    throw new Error("could not resolve a valid source SHA for CP1 profile");
  }
  if (exitCode === 0) {
    if (seedProfiles.some((fixture) => fixture.generator === null)) {
      throw new Error("a successful CP1 measurement is missing a seed profile");
    }
    if (mode === "export" && exportRuns.some((run) => run.summary === null)) {
      throw new Error("a successful CP1 export is missing an export summary");
    }
  }
  await Deno.mkdir(dirname(output), { recursive: true });
  await Deno.writeTextFile(output, `${JSON.stringify(report, null, 2)}\n`);
  console.error(`CP1 profiling report: ${output}`);
}

function required(options: Map<string, string>, key: string): string {
  const value = options.get(key);
  if (!value) throw new Error(`missing required option --${key}`);
  return value;
}

function finiteNumber(value: string | undefined, name: string): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    throw new Error(`invalid ${name}: ${value ?? "missing"}`);
  }
  return parsed;
}

function dirname(path: string): string {
  const index = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return index <= 0 ? "." : path.slice(0, index);
}

async function gitHead(): Promise<string> {
  const command = new Deno.Command("git", {
    args: ["rev-parse", "HEAD"],
    stdout: "piped",
    stderr: "null",
  });
  const result = await command.output();
  if (!result.success) return Deno.env.get("UGOITE_SOURCE_SHA") ?? "";
  const head = new TextDecoder().decode(result.stdout).trim();
  const expected = Deno.env.get("UGOITE_SOURCE_SHA")?.trim();
  if (expected && expected !== head) {
    throw new Error(
      `UGOITE_SOURCE_SHA ${expected} does not match checkout ${head}`,
    );
  }
  return head;
}

async function checkoutDirty(): Promise<boolean | null> {
  const command = new Deno.Command("git", {
    args: ["status", "--porcelain"],
    stdout: "piped",
    stderr: "null",
  });
  const result = await command.output();
  if (!result.success) return null;
  return new TextDecoder().decode(result.stdout).trim().length > 0;
}

async function readJsonOrNull(path: string): Promise<unknown | null> {
  try {
    return JSON.parse(await Deno.readTextFile(path));
  } catch {
    return null;
  }
}

async function findSpacePath(root: string, slug: string): Promise<string> {
  const spaces = `${root.replace(/[\\/]$/, "")}/spaces`;
  try {
    for await (const entry of Deno.readDir(spaces)) {
      if (!entry.isDirectory) continue;
      const path = `${spaces}/${entry.name}`;
      try {
        const metadata = JSON.parse(
          await Deno.readTextFile(`${path}/meta.json`),
        );
        if (metadata?.slug === slug && metadata?.space_uid === entry.name) {
          return path;
        }
      } catch { /* incomplete seed directories are measured as absent */ }
    }
  } catch {
    /* an empty or incomplete root is represented with unavailable metrics */
  }
  return "";
}

export async function fileMetrics(
  root: string,
): Promise<{ file_count: number | null; logical_bytes: number | null }> {
  if (!root) return { file_count: null, logical_bytes: null };
  let fileCount = 0;
  let logicalBytes = 0;
  const pending = [root];
  while (pending.length > 0) {
    const directory = pending.pop()!;
    for await (const entry of Deno.readDir(directory)) {
      const path = `${directory}/${entry.name}`;
      if (entry.isDirectory) pending.push(path);
      else if (entry.isFile) {
        const info = await Deno.stat(path);
        fileCount++;
        logicalBytes += info.size;
      } else if (entry.isSymlink) {
        throw new Error(`unexpected symlink in measured fixture: ${path}`);
      }
    }
  }
  return { file_count: fileCount, logical_bytes: logicalBytes };
}

export async function parseTimeResourceFile(path: string): Promise<{
  elapsed_wall_seconds: number | null;
  user_cpu_seconds: number | null;
  system_cpu_seconds: number | null;
  max_rss_bytes: number | null;
}> {
  let text: string;
  try {
    text = await Deno.readTextFile(path);
  } catch {
    return {
      elapsed_wall_seconds: null,
      user_cpu_seconds: null,
      system_cpu_seconds: null,
      max_rss_bytes: null,
    };
  }
  const linuxWall =
    /Elapsed \(wall clock\) time \(h:mm:ss or m:ss\):\s*([\d:.]+)/i
      .exec(text)?.[1];
  const macWall = /([\d.]+)\s+real\b/i.exec(text)?.[1];
  const user = firstNumber(text, [
    /User time \(seconds\):\s*([\d.]+)/i,
    /user time:\s*([\d.]+)/i,
    /([\d.]+)\s+user\b/i,
  ]);
  const system = firstNumber(text, [
    /System time \(seconds\):\s*([\d.]+)/i,
    /system time:\s*([\d.]+)/i,
    /([\d.]+)\s+sys\b/i,
  ]);
  const rss = firstNumber(text, [
    /Maximum resident set size \(kbytes\):\s*(\d+)/i,
    /maximum resident set size:\s*(\d+)/i,
  ]);
  const rssIsKib = /Maximum resident set size \(kbytes\):/i.test(text);
  return {
    elapsed_wall_seconds: linuxWall
      ? parseElapsed(linuxWall)
      : macWall
      ? Number(macWall)
      : null,
    user_cpu_seconds: user,
    system_cpu_seconds: system,
    max_rss_bytes: rss === null ? null : rssIsKib ? rss * 1024 : rss,
  };
}

function parseElapsed(value: string): number | null {
  const parts = value.split(":").map(Number);
  if (parts.some((part) => !Number.isFinite(part))) return null;
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  if (parts.length === 3) return parts[0] * 3600 + parts[1] * 60 + parts[2];
  return null;
}

export function summarizeMutationBatches(generator: unknown): {
  count: number | null;
  total_micros: number | null;
  p50_micros: number | null;
  p95_micros: number | null;
} {
  if (generator === null || typeof generator !== "object") {
    return {
      count: null,
      total_micros: null,
      p50_micros: null,
      p95_micros: null,
    };
  }
  const values = (generator as Record<string, unknown>).mutation_batch_micros;
  if (
    !Array.isArray(values) ||
    !values.every((value) => typeof value === "number")
  ) {
    return {
      count: null,
      total_micros: null,
      p50_micros: null,
      p95_micros: null,
    };
  }
  const sorted = [...values as number[]].sort((left, right) => left - right);
  return {
    count: sorted.length,
    total_micros: sorted.reduce((total, value) => total + value, 0),
    p50_micros: percentile(sorted, 0.5),
    p95_micros: percentile(sorted, 0.95),
  };
}

export function isCompleteSampleProfile(value: unknown): boolean {
  if (value === null || typeof value !== "object") return false;
  const profile = value as Record<string, unknown>;
  const isNumber = (item: unknown): item is number =>
    typeof item === "number" && Number.isFinite(item) && item >= 0;
  const batchTimes = profile.mutation_batch_micros;
  const batchEntries = profile.mutation_batch_entry_counts;
  return profile.schema_version === 1 &&
    isNumber(profile.space_creation_micros) &&
    (profile.owner_initialization_micros === null ||
      isNumber(profile.owner_initialization_micros)) &&
    isNumber(profile.form_upsert_micros) &&
    isNumber(profile.markdown_render_micros) &&
    isNumber(profile.markdown_render_count) &&
    isNumber(profile.draft_conversion_micros) &&
    isNumber(profile.draft_conversion_count) &&
    isNumber(profile.total_wall_micros) &&
    Array.isArray(batchTimes) && batchTimes.every(isNumber) &&
    Array.isArray(batchEntries) && batchEntries.every(isNumber) &&
    batchTimes.length === batchEntries.length;
}

function percentile(sortedValues: number[], quantile: number): number | null {
  if (sortedValues.length === 0) return null;
  return sortedValues[
    Math.max(0, Math.ceil(sortedValues.length * quantile) - 1)
  ];
}

function sumMeasurements(values: Array<number | null>): number | null {
  if (
    !values.every((value): value is number =>
      typeof value === "number" && Number.isFinite(value)
    )
  ) return null;
  return values.reduce((total, value) => total + value, 0);
}

export function reconcileWallDurations(
  scriptWallMicros: number,
  childProcessWallMicros: number | null,
): {
  unaccounted_script_wall_micros: number | null;
  child_process_wall_excess_micros: number | null;
} {
  if (childProcessWallMicros === null) {
    return {
      unaccounted_script_wall_micros: null,
      child_process_wall_excess_micros: null,
    };
  }
  const delta = scriptWallMicros - childProcessWallMicros;
  return {
    unaccounted_script_wall_micros: Math.max(0, delta),
    child_process_wall_excess_micros: Math.max(0, -delta),
  };
}

function firstNumber(text: string, patterns: RegExp[]): number | null {
  for (const pattern of patterns) {
    const match = pattern.exec(text);
    if (match) return Number(match[1]);
  }
  return null;
}

async function countLines(path: string): Promise<number | null> {
  try {
    const contents = await Deno.readTextFile(path);
    return contents.split("\n").filter((line) => line.length > 0).length;
  } catch {
    return null;
  }
}

async function fileSize(path: string): Promise<number | null> {
  try {
    return (await Deno.stat(path)).size;
  } catch {
    return null;
  }
}

async function fileExists(path: string): Promise<boolean> {
  try {
    await Deno.stat(path);
    return true;
  } catch {
    return false;
  }
}
