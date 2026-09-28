import { assertEquals } from "@std/assert/equals";
import {
  fileMetrics,
  parseTimeResourceFile,
  summarizeMutationBatches,
} from "./cp1_profile.ts";

Deno.test("CP1 profiling parses GNU time CPU and RSS without inventing missing values", async () => {
  const directory = await Deno.makeTempDir();
  try {
    const resourcePath = `${directory}/time.txt`;
    await Deno.writeTextFile(
      resourcePath,
      [
        "User time (seconds): 3.25",
        "System time (seconds): 0.75",
        "Elapsed (wall clock) time (h:mm:ss or m:ss): 0:01.25",
        "Maximum resident set size (kbytes): 4096",
      ].join("\n"),
    );
    assertEquals(await parseTimeResourceFile(resourcePath), {
      elapsed_wall_seconds: 1.25,
      user_cpu_seconds: 3.25,
      system_cpu_seconds: 0.75,
      max_rss_bytes: 4_194_304,
    });
    assertEquals(await parseTimeResourceFile(`${directory}/missing.txt`), {
      elapsed_wall_seconds: null,
      user_cpu_seconds: null,
      system_cpu_seconds: null,
      max_rss_bytes: null,
    });
    await Deno.writeTextFile(
      resourcePath,
      [
        "0.50 real 0.25 user 0.15 sys",
        "maximum resident set size: 8192",
      ].join("\n"),
    );
    assertEquals(await parseTimeResourceFile(resourcePath), {
      elapsed_wall_seconds: 0.5,
      user_cpu_seconds: 0.25,
      system_cpu_seconds: 0.15,
      max_rss_bytes: 8192,
    });
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});

Deno.test("CP1 fixture filesystem metrics count regular files and logical bytes", async () => {
  const directory = await Deno.makeTempDir();
  try {
    await Deno.mkdir(`${directory}/nested`);
    await Deno.writeTextFile(`${directory}/one.json`, "123");
    await Deno.writeTextFile(`${directory}/nested/two.json`, "4567");
    assertEquals(await fileMetrics(directory), {
      file_count: 2,
      logical_bytes: 7,
    });
    assertEquals(await fileMetrics(""), {
      file_count: null,
      logical_bytes: null,
    });
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});

Deno.test("CP1 profiling reports mutation batch p50 and p95 and keeps missing data null", () => {
  assertEquals(
    summarizeMutationBatches({ mutation_batch_micros: [9, 1, 5, 4, 5] }),
    { count: 5, total_micros: 24, p50_micros: 5, p95_micros: 9 },
  );
  assertEquals(summarizeMutationBatches(null), {
    count: null,
    total_micros: null,
    p50_micros: null,
    p95_micros: null,
  });
});

Deno.test("successful CP1 profile aggregation keeps fixture, resource, and source evidence", async () => {
  const directory = await Deno.makeTempDir();
  try {
    const root = `${directory}/fixture-root`;
    const spaceUid = "019f0000-0000-7000-8000-000000000001";
    const spacePath = `${root}/spaces/${spaceUid}`;
    await Deno.mkdir(spacePath, { recursive: true });
    await Deno.writeTextFile(
      `${spacePath}/meta.json`,
      JSON.stringify({ slug: "query-space-a", space_uid: spaceUid }),
    );
    const seedProfile = `${directory}/seed.json`;
    await Deno.writeTextFile(
      seedProfile,
      JSON.stringify({
        schema_version: 1,
        space_slug: "query-space-a",
        scenario: "renewable-ops",
        seed: 3134001,
        entry_count: 6000,
        form_count: 5,
        space_creation_micros: 2,
        owner_initialization_micros: 3,
        form_upsert_micros: 4,
        markdown_render_micros: 5,
        draft_conversion_micros: 6,
        mutation_batch_micros: [7, 9],
        total_wall_micros: 50,
      }),
    );
    const seedResources = `${directory}/seed.time.txt`;
    await Deno.writeTextFile(
      seedResources,
      [
        "User time (seconds): 1.2",
        "System time (seconds): 0.2",
        "Elapsed (wall clock) time (h:mm:ss or m:ss): 0:01.5",
        "Maximum resident set size (kbytes): 2048",
      ].join("\n"),
    );
    const buildResources = `${directory}/build.time.txt`;
    await Deno.writeTextFile(buildResources, "0.5 real 0.4 user 0.1 sys\n");
    const output = `${directory}/report.json`;
    const command = new Deno.Command(Deno.execPath(), {
      args: [
        "run",
        "-A",
        new URL("./cp1_profile.ts", import.meta.url).pathname,
        "query",
        "--output",
        output,
        "--root",
        root,
        "--started-ms",
        String(Date.now() - 1500),
        "--exit-code",
        "0",
        "--seed",
        "query-space-a",
        "3134001",
        "6000",
        "Query Measurement Owner",
        seedProfile,
        seedResources,
        "--timed-step",
        "wasm-build",
        buildResources,
      ],
      stdout: "piped",
      stderr: "piped",
    });
    const result = await command.output();
    assertEquals(
      result.success,
      true,
      new TextDecoder().decode(result.stderr),
    );
    const report = JSON.parse(await Deno.readTextFile(output));
    assertEquals(report.source_sha.length, 40);
    assertEquals(report.result, { exit_code: 0, successful: true });
    assertEquals(report.seed_process_invocations, 1);
    assertEquals(report.fixtures[0].filesystem, {
      file_count: 1,
      logical_bytes: new TextEncoder().encode(
        JSON.stringify({ slug: "query-space-a", space_uid: spaceUid }),
      ).length,
    });
    assertEquals(report.fixtures[0].mutation_batch_summary, {
      count: 2,
      total_micros: 16,
      p50_micros: 7,
      p95_micros: 9,
    });
    assertEquals(report.process_steps[0].process.elapsed_wall_seconds, 0.5);
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
});
