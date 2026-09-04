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

function workflowStepBlock(source: string, step: string): string {
  const header = `\n      - name: ${step}\n`;
  const start = source.indexOf(header);
  assertEquals(start >= 0, true, `workflow is missing step ${step}`);
  const nextStep = source.slice(start + header.length).search(
    /^\x20{6}- name: /m,
  );
  return source.slice(
    start + header.length,
    nextStep < 0 ? undefined : start + header.length + nextStep,
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
  const rustCheckJob = workflowJobBlock(workflow, "rust-check");
  const rustTestJob = workflowJobBlock(workflow, "rust-test");
  const webJob = workflowJobBlock(workflow, "web");
  const artifactsJob = workflowJobBlock(workflow, "artifacts");
  const requiredJob = workflowJobBlock(workflow, "required");
  const rustCheckCargoCache = workflowStepBlock(
    rustCheckJob,
    "Restore Cargo dependency cache",
  );
  const rustTestCargoCache = workflowStepBlock(
    rustTestJob,
    "Restore Cargo dependency cache",
  );
  const webCargoCache = workflowStepBlock(
    webJob,
    "Restore Cargo dependency cache",
  );
  const artifactsCargoCache = workflowStepBlock(
    artifactsJob,
    "Restore Cargo dependency cache",
  );
  const canonicalCi = taskBlock(mise, "ci");
  const rustLint = taskBlock(mise, "lint:rust");
  const denoLint = taskBlock(mise, "lint:deno");
  const canonicalLint = taskBlock(mise, "lint");
  const rustCheck = taskBlock(mise, "check:rust");
  const denoCheck = taskBlock(mise, "check:deno");
  const repoCheck = taskBlock(mise, "check:repo");
  const canonicalCheck = taskBlock(mise, "check");
  const canonicalTest = taskBlock(mise, "test");
  const rustCheckLane = taskBlock(mise, "ci:lane:rust-check");
  const rustTestLane = taskBlock(mise, "ci:lane:rust-test");
  const webLane = taskBlock(mise, "ci:lane:web");
  const releaseBuild = taskBlock(mise, "build:rust:release");
  const artifactsTask = taskBlock(mise, "ci:artifacts");
  const artifactsE2eTask = taskBlock(mise, "ci:artifacts:e2e");
  const mergeTask = taskBlock(mise, "ci:merge");

  assertContainsAll(
    canonicalCi,
    [
      '{ task = "fmt:check" }',
      '{ task = "lint" }',
      '{ task = "check" }',
      '{ task = "test" }',
    ],
    "canonical CI task",
  );
  assertContainsAll(
    rustLint,
    ["cargo clippy --workspace --all-targets --all-features -- -D warnings"],
    "Rust lint task",
  );
  assertContainsAll(
    denoLint,
    ["deno lint tools e2e frontend/src docsite/src"],
    "Deno lint task",
  );
  assertContainsAll(
    canonicalLint,
    ['{ task = "lint:rust" }', '{ task = "lint:deno" }'],
    "canonical lint task",
  );
  assertContainsAll(
    rustCheck,
    [
      "cargo check --workspace --all-targets --all-features --locked",
      "cargo check -p ugoite-domain --target wasm32-unknown-unknown --locked",
      "cargo check -p ugoite-api-client --target wasm32-unknown-unknown --locked",
      "cargo check -p ugoite-wasm --target wasm32-unknown-unknown --locked",
    ],
    "Rust check task",
  );
  assertContainsAll(denoCheck, ["deno task check"], "Deno check task");
  assertContainsAll(
    repoCheck,
    [
      "cargo run -p xtask -- openapi-check",
      "cargo run -p xtask -- architecture-check",
      "cargo run -p xtask -- docs-current-stack-check",
      '{ task = "check:supported" }',
      "cargo run -p xtask -- legacy-auth-check",
    ],
    "repository check task",
  );
  assertContainsAll(
    canonicalCheck,
    [
      '{ task = "check:rust" }',
      '{ task = "check:deno" }',
      '{ task = "check:repo" }',
    ],
    "canonical check task",
  );
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
    [
      "crates/ugoite-identity/**/*",
      "cargo build -p ugoite-server -p ugoite-cli --release --locked",
    ],
    "release build task",
  );
  assertContainsAll(
    artifactsTask,
    [
      '{ task = "build" }',
      '{ task = "package" }',
      '{ task = "verify" }',
      '{ task = "ci:artifacts:e2e" }',
    ],
    "artifact CI task",
  );
  assertContainsAll(
    artifactsE2eTask,
    [
      '{ task = "test:docsite:e2e:navigation" }',
      '{ task = "test:e2e:smoke-and-asset-owned" }',
      '{ task = "test:e2e:owner-recovery" }',
      '{ task = "version:check" }',
    ],
    "artifact E2E task",
  );
  assertContainsAll(
    mergeTask,
    ['{ task = "ci" }', '{ task = "ci:artifacts" }'],
    "merge CI task",
  );

  const hostedLaneExpectations: [string, string[]][] = [
    ["rust-check", ["lint:rust", "check:rust", "check:repo"]],
    ["rust-test", ["test:rust"]],
    [
      "web",
      [
        "fmt:check",
        "lint:deno",
        "check:deno",
        "test:tools",
        "test:frontend:coverage",
        "test:docsite:coverage",
      ],
    ],
  ];
  for (const [lane, expectedTasks] of hostedLaneExpectations) {
    const laneBlock = taskBlock(mise, `ci:lane:${lane}`);
    assertContainsAll(
      laneBlock,
      expectedTasks.map((task) => `"${task}"`),
      `hosted ${lane} lane`,
    );
  }
  assertEquals(
    rustCheckLane.includes("cargo ") || rustCheckLane.includes("deno ") ||
      rustCheckLane.includes("rustup ") || rustCheckLane.includes("sccache "),
    false,
    "hosted rust-check lane must compose tasks instead of running commands",
  );
  assertEquals(
    rustTestLane.includes("cargo ") || rustTestLane.includes("deno ") ||
      rustTestLane.includes("rustup ") || rustTestLane.includes("sccache "),
    false,
    "hosted rust-test lane must compose tasks instead of running commands",
  );
  assertEquals(
    webLane.includes("cargo ") || webLane.includes("deno ") ||
      webLane.includes("rustup ") || webLane.includes("sccache "),
    false,
    "hosted web lane must compose tasks instead of running commands",
  );

  assertContainsAll(
    rustCheckJob,
    [
      "name: ci-rust-check",
      "scripts/measure-step.sh rust-check mise run ci:lane:rust-check",
      "sccache --show-stats",
    ],
    "Rust check CI lane",
  );
  assertContainsAll(
    rustTestJob,
    [
      "name: ci-rust-test",
      "scripts/measure-step.sh rust-test mise run ci:lane:rust-test",
      "sccache --show-stats",
    ],
    "Rust test CI lane",
  );
  assertContainsAll(
    webJob,
    [
      "name: ci-web",
      "scripts/measure-step.sh web mise run ci:lane:web",
      "sccache --show-stats",
      "- name: Save Deno cache",
    ],
    "web CI lane",
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
    workflow.includes("mise run ci\n"),
    false,
    "CI must not invoke the canonical aggregate task directly",
  );
  assertEquals(
    workflow.includes("MISE_JOBS"),
    false,
    "CI must not impose workflow-global Mise parallelism",
  );
  assertEquals(
    rustCheckJob.includes("Restore Deno cache"),
    false,
    "Rust check lane must not restore the Deno archive",
  );
  assertEquals(
    rustTestJob.includes("Restore Deno cache"),
    false,
    "Rust test lane must not restore the Deno archive",
  );
  assertContainsAll(
    rustCheckCargoCache,
    [
      "save-if: ${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}",
    ],
    "Rust check Cargo dependency cache writer",
  );
  for (
    const [cache, subject] of [
      [rustTestCargoCache, "Rust test Cargo dependency cache"],
      [webCargoCache, "web Cargo dependency cache"],
      [artifactsCargoCache, "artifact Cargo dependency cache"],
    ]
  ) {
    assertContainsAll(cache, ['save-if: "false"'], subject);
  }
  assertContainsAll(
    workflowStepBlock(webJob, "Restore Deno cache"),
    ["actions/cache/restore", "path: ${{ runner.temp }}/deno-cache"],
    "web Deno archive restore",
  );
  assertContainsAll(
    workflowStepBlock(artifactsJob, "Restore Deno cache"),
    ["actions/cache/restore", "path: ${{ runner.temp }}/deno-cache"],
    "artifact Deno archive restore",
  );
  assertEquals(
    artifactsJob.includes("- name: Save Deno cache"),
    false,
    "artifact lane must not write the Deno archive",
  );
  assertEquals(
    workflow.match(/mise run [A-Za-z0-9:_-]+/g)?.sort().join("\n"),
    [
      "mise run ci:artifacts",
      "mise run ci:lane:rust-check",
      "mise run ci:lane:rust-test",
      "mise run ci:lane:web",
    ].sort().join("\n"),
    "CI must invoke only Hosted lane and artifact Mise entrypoints",
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
      "needs: [rust-check, rust-test, web, artifacts]",
      "RUST_CHECK_RESULT: ${{ needs.rust-check.result }}",
      "RUST_TEST_RESULT: ${{ needs.rust-test.result }}",
      "WEB_RESULT: ${{ needs.web.result }}",
      "ARTIFACTS_RESULT: ${{ needs.artifacts.result }}",
      'test "$RUST_CHECK_RESULT" = success',
      'test "$RUST_TEST_RESULT" = success',
      'test "$WEB_RESULT" = success',
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
