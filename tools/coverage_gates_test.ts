import { assertEquals } from "@std/assert/equals";

const frontendTasks = JSON.parse(
  await Deno.readTextFile("frontend/deno.json"),
);
const docsiteTasks = JSON.parse(
  await Deno.readTextFile("docsite/deno.json"),
);

function taskBlock(source: string, task: string): string {
  const header = `[tasks."${task}"]`;
  const start = source.indexOf(header);
  assertEquals(start >= 0, true, `missing mise task ${task}`);
  const end = source.indexOf("\n[tasks", start + header.length);
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
    /^  [A-Za-z0-9_-]+:\n/m,
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

Deno.test("REQ-OPS-021: frontend coverage remains a required CI contract", async () => {
  const frontendConfig = await Deno.readTextFile("frontend/vitest.config.ts");
  const rootDeno = await Deno.readTextFile("deno.json");
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");
  const requiredJob = workflowJobBlock(workflow, "required");
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
    requiredJob,
    [
      "name: ci-required",
      "id: test-frontend-coverage",
      "scripts/measure-step.sh test-frontend-coverage mise run test:frontend:coverage",
    ],
    "required CI frontend coverage gate",
  );
  assertMainTrigger(workflow, "pull_request");
  assertMainTrigger(workflow, "merge_group");
  assertMainTrigger(workflow, "push");
  assertEquals(
    requiredJob.includes("continue-on-error:"),
    false,
    "frontend coverage must fail the required job",
  );
  assertContainsAll(
    requirementBlock(requirements, "REQ-OPS-021"),
    [
      "status: implemented",
      "verification: traced",
      "- file: tools/coverage_gates_test.ts",
    ],
    "REQ-OPS-021",
  );
});

Deno.test("REQ-OPS-024: docsite coverage remains a required CI contract", async () => {
  const docsiteConfig = await Deno.readTextFile("docsite/vitest.config.ts");
  const rootDeno = await Deno.readTextFile("deno.json");
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");
  const requiredJob = workflowJobBlock(workflow, "required");
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
  assertEquals(
    taskBlock(mise, "test:docsite:coverage").includes(
      "deno task docsite:coverage",
    ),
    true,
  );
  assertContainsAll(
    requiredJob,
    [
      "id: test-docsite-coverage",
      "scripts/measure-step.sh test-docsite-coverage mise run test:docsite:coverage",
    ],
    "required CI docsite coverage gate",
  );
  assertEquals(
    requiredJob.includes("continue-on-error:"),
    false,
    "docsite coverage must fail the required job",
  );
  assertContainsAll(
    requirementBlock(requirements, "REQ-OPS-024"),
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
