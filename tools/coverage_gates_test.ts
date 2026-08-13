import { assertEquals } from "@std/assert/equals";

const frontendTasks = JSON.parse(
  await Deno.readTextFile("frontend/deno.json"),
);
const docsiteTasks = JSON.parse(
  await Deno.readTextFile("docsite/deno.json"),
);

function taskBlock(source: string, task: string): string {
  const header = [`[tasks."${task}"]`, `[tasks.${task}]`].find((candidate) =>
    source.includes(candidate)
  );
  assertEquals(header === undefined, false, `missing mise task ${task}`);
  const start = source.indexOf(header as string);
  const end = source.indexOf("\n[tasks", start + (header as string).length);
  return source.slice(start, end === -1 ? undefined : end);
}

function assertContainsAll(
  source: string,
  snippets: string[],
  subject: string,
): void {
  for (const snippet of snippets) {
    assertEquals(
      source.includes(snippet),
      true,
      `${subject} is missing ${JSON.stringify(snippet)}`,
    );
  }
}

function workflowJobBlock(source: string, job: string): string {
  const jobsStart = source.indexOf("\njobs:\n");
  assertEquals(jobsStart >= 0, true, "workflow is missing jobs");
  const jobHeader = `\n  ${job}:\n`;
  const jobStart = source.indexOf(jobHeader, jobsStart);
  assertEquals(jobStart >= 0, true, `workflow is missing jobs.${job}`);
  const nextJob = source.slice(jobStart + jobHeader.length).search(
    /^\x20{2}[A-Za-z0-9_-]+:\n/m,
  );
  return source.slice(
    jobStart + jobHeader.length,
    nextJob < 0 ? undefined : jobStart + jobHeader.length + nextJob,
  );
}

function assertMainTrigger(source: string, trigger: string): void {
  assertContainsAll(
    source,
    [`${trigger}:\n    branches:\n      - main`],
    `${trigger} trigger`,
  );
}

function assertAggregateWorkflow(workflow: string, mise: string): void {
  const qualityJob = workflowJobBlock(workflow, "quality");
  const artifactsJob = workflowJobBlock(workflow, "artifacts");
  const requiredJob = workflowJobBlock(workflow, "required");
  const canonicalTest = taskBlock(mise, "test");
  const releaseBuild = taskBlock(mise, "build:rust:release");
  const artifactsTask = taskBlock(mise, "ci:artifacts");
  const mergeTask = taskBlock(mise, "ci:merge");

  assertContainsAll(
    canonicalTest,
    [
      "test:rust",
      "test:tools",
      "test:frontend:coverage",
      "test:docsite:coverage",
    ],
    "canonical test task",
  );
  assertEquals(
    canonicalTest.includes("test:frontend:after-wasm"),
    false,
    "canonical test task must not run the focused frontend suite",
  );
  assertEquals(
    canonicalTest.includes('"test:docsite",'),
    false,
    "canonical test task must not run the focused docsite suite",
  );

  assertContainsAll(
    releaseBuild,
    ["cargo build -p ugoite-server -p ugoite-cli --release --locked"],
    "release build task",
  );
  assertContainsAll(
    artifactsTask,
    [
      '{ task = "build" }',
      '{ task = "package" }',
      '{ task = "verify" }',
      '"test:docsite:e2e:navigation"',
      '"test:e2e:smoke-and-asset-owned"',
      '"validate:release"',
    ],
    "artifact CI task",
  );
  assertContainsAll(
    mergeTask,
    ['{ task = "ci" }', '{ task = "ci:artifacts" }'],
    "merge CI task",
  );

  assertContainsAll(
    qualityJob,
    ["name: quality", "scripts/measure-step.sh quality mise run ci"],
    "quality CI lane",
  );
  assertContainsAll(
    artifactsJob,
    [
      "name: artifacts",
      "scripts/measure-step.sh artifacts mise run ci:artifacts",
    ],
    "artifact CI lane",
  );
  assertEquals(
    qualityJob.includes("mise run test:frontend:coverage"),
    false,
    "quality lane must not know individual coverage tasks",
  );
  assertEquals(
    qualityJob.includes("mise run test:docsite:coverage"),
    false,
    "quality lane must not know individual coverage tasks",
  );
  assertEquals(
    qualityJob.includes("Playwright"),
    false,
    "quality lane must not install or configure Playwright",
  );
  assertEquals(
    qualityJob.includes("Buildx"),
    false,
    "quality lane must not configure Buildx",
  );
  assertEquals(
    workflow.includes("mise run test:frontend:coverage"),
    false,
    "CI must not invoke the frontend coverage task directly",
  );
  assertEquals(
    workflow.includes("mise run test:docsite:coverage"),
    false,
    "CI must not invoke the docsite coverage task directly",
  );

  assertContainsAll(
    requiredJob,
    [
      "name: ci-required",
      "if: ${{ always() }}",
      "needs: [quality, artifacts]",
      "QUALITY_RESULT: ${{ needs.quality.result }}",
      "ARTIFACTS_RESULT: ${{ needs.artifacts.result }}",
      'test "$QUALITY_RESULT" = success',
      'test "$ARTIFACTS_RESULT" = success',
    ],
    "required CI aggregator",
  );
  assertEquals(
    requiredJob.includes("actions/checkout"),
    false,
    "required CI aggregator must not duplicate a lane",
  );

  assertMainTrigger(workflow, "pull_request");
  assertMainTrigger(workflow, "merge_group");
  assertMainTrigger(workflow, "push");
}

Deno.test("CI aggregate tasks own test coverage and lane scheduling", async () => {
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");

  assertAggregateWorkflow(workflow, mise);
});

Deno.test("REQ-OPS-021: frontend coverage remains a canonical test contract", async () => {
  const frontendConfig = await Deno.readTextFile("frontend/vitest.config.ts");
  const rootDeno = await Deno.readTextFile("deno.json");
  const mise = await Deno.readTextFile("mise.toml");
  const requirements = await Deno.readTextFile(
    "docs/spec/requirements/ops.yaml",
  );

  assertEquals(
    frontendTasks.tasks.coverage,
    "deno run -A npm:vitest run --coverage --maxWorkers=1",
  );
  assertContainsAll(
    frontendConfig,
    [
      'provider: "v8"',
      'include: ["src/lib/ugoite-client/protocol.ts"]',
      "lines: 100",
      "functions: 100",
      "branches: 100",
      "statements: 100",
    ],
    "frontend coverage config",
  );
  assertEquals(
    rootDeno.includes(
      '"frontend:coverage": "deno task --cwd frontend coverage"',
    ),
    true,
  );
  assertContainsAll(
    taskBlock(mise, "test:frontend:coverage"),
    [
      "build:wasm:debug",
      "scripts/activate-ugoite-wasm.sh debug",
      "deno task frontend:coverage",
    ],
    "frontend root coverage task",
  );
  assertContainsAll(
    taskBlock(mise, "test"),
    ["test:frontend:coverage"],
    "canonical test task",
  );
  assertContainsAll(
    requirementBlock(
      requirements,
      "REQ-OPS-021",
    ),
    [
      "status: implemented",
      "verification: traced",
      "- file: tools/coverage_gates_test.ts",
    ],
    "REQ-OPS-021",
  );
});

Deno.test("REQ-OPS-024: docsite coverage remains a canonical test contract", async () => {
  const docsiteConfig = await Deno.readTextFile("docsite/vitest.config.ts");
  const rootDeno = await Deno.readTextFile("deno.json");
  const mise = await Deno.readTextFile("mise.toml");
  const requirements = await Deno.readTextFile(
    "docs/spec/requirements/ops.yaml",
  );

  assertEquals(
    docsiteTasks.tasks.coverage,
    "deno run -A npm:vitest@4.1.8 run --coverage --maxWorkers=1",
  );
  assertContainsAll(
    docsiteConfig,
    [
      'include: ["src/**/*.{js,mjs,ts,tsx}"]',
      '"src/**/*.test.*"',
      '"src/**/*.spec.*"',
      '"src/env.d.ts"',
      '"src/content.config.ts"',
      'provider: "v8"',
      "lines: 100",
      "functions: 100",
      "branches: 100",
      "statements: 100",
    ],
    "docsite coverage config",
  );
  assertEquals(
    rootDeno.includes('"docsite:coverage": "deno task --cwd docsite coverage"'),
    true,
  );
  assertContainsAll(
    taskBlock(mise, "test:docsite:coverage"),
    ["deno task docsite:coverage"],
    "docsite root coverage task",
  );
  assertContainsAll(
    taskBlock(mise, "test"),
    ["test:docsite:coverage"],
    "canonical test task",
  );
  assertContainsAll(
    requirementBlock(
      requirements,
      "REQ-OPS-024",
    ),
    [
      "status: implemented",
      "verification: traced",
      "- file: tools/coverage_gates_test.ts",
    ],
    "REQ-OPS-024",
  );
});

function requirementBlock(source: string, id: string): string {
  const start = source.indexOf(`  id: ${id}`);
  assertEquals(start >= 0, true, `missing requirement ${id}`);
  const end = source.indexOf("\n- set_id:", start);
  return source.slice(start, end === -1 ? undefined : end);
}
