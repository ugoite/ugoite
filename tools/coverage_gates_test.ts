import { assertEquals } from "@std/assert/equals";

const frontendTasks = JSON.parse(
  await Deno.readTextFile("frontend/deno.json"),
);
const docsiteTasks = JSON.parse(
  await Deno.readTextFile("docsite/deno.json"),
);
const ciGateScript = new URL("../scripts/ci-gate-check.sh", import.meta.url);

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

async function ciGatePasses(
  overrides: Record<string, string>,
): Promise<boolean> {
  const environment: Record<string, string> = {
    EVENT_NAME: "pull_request",
    EVENT_ACTION: "edited",
    PR_BASE_CHANGED: "true",
    IMPACT_RESULT: "success",
    IMPACT_PLAN_STATUS: "ok",
    IMPACT_PLAN_SCOPE: "all",
    IMPACT_JOBS_SKIPPED: "false",
    RUST_CHECK_RESULT: "success",
    RUST_TEST_RESULT: "success",
    S3_SHARED_AUTHORIZATION_RESULT: "success",
    WEB_RESULT: "success",
    ARTIFACT_BUILD_RESULT: "success",
    E2E_SMOKE_MOBILE_RESULT: "success",
    E2E_OWNER_RESULT: "success",
    E2E_PORTABLE_RESULT: "success",
    DOCSITE_NAV_RESULT: "success",
    CP1_FIXTURES_RESULT: "success",
    CP1_QUERY_RESULT: "success",
    CP1_EXPORT_RESULT: "success",
    PLAN_RUST_CHECK: "true",
    PLAN_RUST_TEST: "true",
    PLAN_WEB: "true",
    PLAN_ARTIFACTS: "true",
    PLAN_DOCSITE_NAV: "true",
    PLAN_CP1_ACCEPTANCE: "true",
    PR_CONTEXT_RESULT: "success",
    ...overrides,
  };
  const result = await new Deno.Command("bash", {
    args: [ciGateScript.pathname],
    env: environment,
    stdout: "null",
    stderr: "null",
  }).output();
  return result.success;
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

async function assertAggregateWorkflow(
  workflow: string,
  mise: string,
): Promise<void> {
  const rustCheckJob = workflowJobBlock(workflow, "rust-check");
  const rustTestJob = workflowJobBlock(workflow, "rust-test");
  const s3SharedAuthorizationJob = workflowJobBlock(
    workflow,
    "s3-shared-authorization",
  );
  const webJob = workflowJobBlock(workflow, "web");
  const artifactBuildJob = workflowJobBlock(workflow, "artifact-build");
  const e2eSmokeMobileJob = workflowJobBlock(workflow, "e2e-smoke-mobile");
  const e2eOwnerJob = workflowJobBlock(workflow, "e2e-owner");
  const e2ePortableJob = workflowJobBlock(workflow, "e2e-portable");
  const impactJob = workflowJobBlock(workflow, "impact");
  const docsiteNavJob = workflowJobBlock(workflow, "docsite-nav");
  const cp1FixturesJob = workflowJobBlock(workflow, "cp1-fixtures");
  const cp1QueryJob = workflowJobBlock(workflow, "cp1-query");
  const cp1ExportJob = workflowJobBlock(workflow, "cp1-export");
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
    artifactBuildJob,
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
  const artifactsPrepareTask = taskBlock(mise, "ci:artifacts:prepare");
  const artifactsE2eTask = taskBlock(mise, "ci:artifacts:e2e");
  const artifactLoadTask = taskBlock(mise, "ci:artifacts:load");
  const releaseCliSeed = await Deno.readTextFile(
    new URL("../e2e/scripts/seed-portable-space.sh", import.meta.url),
  );
  const measureStep = await Deno.readTextFile(
    new URL("../scripts/measure-step.sh", import.meta.url),
  );
  const sqlExportMeasure = await Deno.readTextFile(
    new URL("../scripts/measure-sql-export.sh", import.meta.url),
  );
  const querySurfaceMeasure = await Deno.readTextFile(
    new URL("../scripts/measure-query-surfaces.sh", import.meta.url),
  );
  const queryBrowserTest = await Deno.readTextFile(
    new URL("../e2e/query-surfaces-measurement.test.ts", import.meta.url),
  );
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
      "test:docsite",
    ],
    "canonical test task",
  );
  assertEquals(
    canonicalTest.includes("test:frontend:after-wasm"),
    false,
    "canonical test task must not run the focused frontend suite",
  );
  assertEquals(
    canonicalTest.includes('"test:docsite:coverage"'),
    false,
    "canonical test task must not run docsite coverage instrumentation",
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
      '{ task = "ci:artifacts:prepare" }',
      '{ task = "ci:artifacts:e2e" }',
    ],
    "artifact CI task",
  );
  assertContainsAll(
    artifactsPrepareTask,
    [
      '{ task = "build" }',
      '{ task = "package" }',
      '{ task = "verify" }',
      '{ task = "version:check" }',
    ],
    "artifact preparation task",
  );
  assertContainsAll(
    artifactLoadTask,
    ["tools/ci_artifact_bundle.ts load"],
    "E2E artifact verification and load task",
  );
  assertContainsAll(
    artifactsE2eTask,
    [
      '{ task = "test:e2e:portable-space", env = { UGOITE_PORTABLE_CLI_BINARY = "target/rust/release/ugoite" } }',
    ],
    "portable E2E uses the release CLI built by the artifact lane",
  );
  assertContainsAll(
    releaseBuild,
    [
      '"target/rust/release/ugoite.source-sha"',
      "target/rust/release/ugoite.source-sha",
    ],
    "release CLI source identity output",
  );
  assertContainsAll(
    releaseBuild,
    ["record:rust:source-sha"],
    "release build refreshes CLI provenance before fingerprinting",
  );
  assertContainsAll(
    taskBlock(mise, "record:rust:source-sha"),
    [
      "target/rust/release/ugoite.source-sha",
      "UGOITE_SOURCE_SHA",
      "cmp -s",
    ],
    "release CLI provenance refreshes without rebuilding",
  );
  assertContainsAll(
    releaseCliSeed,
    [
      'PORTABLE_CLI_BINARY="${UGOITE_PORTABLE_CLI_BINARY:-}"',
      '[[ ! -x "$PORTABLE_CLI_BINARY" ]]',
      '[[ "$(cat "$source_sha_file")" != "$CHECKOUT_SOURCE_SHA" ]]',
      'run_cli "$config" space verify --deep --format json',
      "cargo run -q --manifest-path",
      "portable_seed_duration_seconds",
    ],
    "portable fixture CLI selection and provenance checks",
  );
  assertEquals(
    (releaseCliSeed.match(/cargo run -q --manifest-path/g) ?? []).length,
    1,
    "all fixture commands use the selected CLI runner instead of recompiling",
  );
  assertContainsAll(
    measureStep,
    ["trap finish_measurement EXIT", "duration_seconds=", "exit_code="],
    "CI measurement preserves failed-step duration and status",
  );
  assertContainsAll(
    artifactsE2eTask,
    [
      '{ task = "test:docsite:e2e:navigation" }',
      '{ task = "test:e2e:smoke-and-asset-owned" }',
      '{ task = "test:e2e:mobile-ui" }',
      '{ task = "test:e2e:owner-recovery" }',
      '{ task = "test:e2e:portable-space", env = { UGOITE_PORTABLE_CLI_BINARY = "target/rust/release/ugoite" } }',
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
        "test:docsite",
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
    artifactBuildJob,
    [
      "name: artifact-build",
      "scripts/measure-step.sh artifact-prepare mise run ci:artifacts:prepare",
      "name: Upload runtime image artifact",
      "name: ugoite-runtime-image",
      "name: Upload CLI artifact",
      "name: ugoite-cli-linux",
      "name: Upload artifact manifest",
      "name: ugoite-artifact-manifest",
      "github.event_name == 'pull_request'",
      "github.event_name == 'merge_group'",
      "github.event_name == 'push' && github.ref == 'refs/heads/main'",
      "E2E_ARTIFACT_INPUT_BYTES",
    ],
    "artifact build CI lane",
  );
  assertEquals(
    artifactBuildJob.includes("ugoite-ci-e2e-inputs"),
    false,
    "artifact build must not upload a duplicate combined E2E bundle",
  );
  for (
    const [step, subject] of [
      ["Upload runtime image artifact", "runtime image artifact upload"],
      ["Upload CLI artifact", "CLI artifact upload"],
      ["Upload artifact manifest", "artifact manifest upload"],
    ]
  ) {
    assertContainsAll(
      workflowStepBlock(artifactBuildJob, step),
      ["retention-days: 14"],
      `${subject} retention`,
    );
  }
  assertContainsAll(
    e2eSmokeMobileJob,
    [
      "name: ci-e2e-smoke-mobile",
      "needs: [impact, artifact-build]",
      "UGOITE_ARTIFACT_SELECTION: runtime",
      "name: ugoite-artifact-manifest",
      "name: ugoite-runtime-image",
      "name: Configure Deno and Playwright cache paths",
      'echo "DENO_DIR=${RUNNER_TEMP}/deno-cache" >>"$GITHUB_ENV"',
      "actions/download-artifact@",
      "scripts/measure-step.sh load-artifacts mise run ci:artifacts:load",
      "scripts/measure-step.sh smoke-mobile mise run ci:lane:e2e-smoke-mobile",
      "DOWNLOAD_BYTES",
    ],
    "smoke/mobile E2E consumer lane",
  );
  for (
    const [job, subject] of [
      [e2eSmokeMobileJob, "smoke/mobile E2E consumer lane"],
      [e2eOwnerJob, "owner recovery E2E consumer lane"],
      [e2ePortableJob, "portable-space E2E consumer lane"],
    ] as const
  ) {
    assertContainsAll(
      job,
      [
        "runs-on: ubuntu-24.04",
        "scripts/measure-step.sh browser-deps deno task e2e:install:browsers",
        "BROWSER_DEPS_SECONDS",
      ],
      `${subject} records pinned-runner browser setup duration`,
    );
  }
  assertContainsAll(
    e2eOwnerJob,
    [
      "name: ci-e2e-owner",
      "UGOITE_ARTIFACT_SELECTION: runtime",
      "name: ugoite-artifact-manifest",
      "name: ugoite-runtime-image",
      "name: Configure Deno and Playwright cache paths",
      "actions/download-artifact@",
      "scripts/measure-step.sh owner-recovery mise run ci:lane:e2e-owner",
    ],
    "owner recovery E2E consumer lane",
  );
  assertContainsAll(
    e2ePortableJob,
    [
      "name: ci-e2e-portable",
      "UGOITE_ARTIFACT_SELECTION: both",
      "name: ugoite-artifact-manifest",
      "name: ugoite-runtime-image",
      "name: ugoite-cli-linux",
      "name: Configure Deno and Playwright cache paths",
      "actions/download-artifact@",
      "scripts/measure-step.sh portable-space mise run ci:lane:e2e-portable",
    ],
    "portable-space E2E consumer lane",
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
    workflowStepBlock(artifactBuildJob, "Restore Deno cache"),
    ["actions/cache/restore", "path: ${{ runner.temp }}/deno-cache"],
    "artifact Deno archive restore",
  );
  assertEquals(
    artifactBuildJob.includes("- name: Save Deno cache"),
    false,
    "artifact lane must not write the Deno archive",
  );
  assertEquals(
    workflow.match(/mise run [A-Za-z0-9:_-]+/g)?.sort().join("\n"),
    [
      "mise run ci:artifacts:load",
      "mise run ci:artifacts:load",
      "mise run ci:artifacts:load",
      "mise run ci:artifacts:load",
      "mise run ci:artifacts:load",
      "mise run ci:artifacts:prepare",
      "mise run ci:impact",
      "mise run ci:cp1:fixtures",
      "mise run ci:lane:cp1-export",
      "mise run ci:lane:cp1-query",
      "mise run ci:lane:e2e-portable",
      "mise run ci:lane:e2e-smoke-mobile",
      "mise run ci:lane:docsite-nav",
      "mise run ci:lane:e2e-owner",
      "mise run ci:lane:rust-check",
      "mise run ci:lane:rust-test",
      "mise run test:s3-storage",
      "mise run ci:lane:web",
    ].sort().join("\n"),
    "CI must invoke only Hosted lane and artifact Mise entrypoints",
  );
  assertContainsAll(
    s3SharedAuthorizationJob,
    [
      "name: ci-s3-shared-authorization",
      "needs: impact",
      "needs.impact.outputs.plan_rust_test == 'true'",
      "pgsty/minio@sha256:",
      "UGOITE_S3_TEST_ENDPOINT",
      "UGOITE_S3_TEST_BUCKET",
      'UGOITE_S3_TEST_REQUIRED: "1"',
      "mise run test:s3-storage",
    ],
    "S3 shared-authorization acceptance lane",
  );
  assertContainsAll(
    impactJob,
    [
      "name: ci-impact-plan",
      "fetch-depth: 0",
      "CI_IMPACT_EVENT: ${{ github.event_name }}",
      "github.event.pull_request.base.sha || github.event.before",
      "github.event.pull_request.head.sha || github.sha",
      "install_args: deno",
      "run: mise run ci:impact",
    ],
    "impact plan lane",
  );
  assertContainsAll(
    docsiteNavJob,
    [
      "name: ci-docsite-nav",
      "scripts/measure-step.sh docsite-nav mise run ci:lane:docsite-nav",
      "test-results/docsite-navigation-junit.xml",
      "install_args: deno",
    ],
    "standalone docsite navigation lane",
  );
  assertEquals(
    docsiteNavJob.includes("Install Rust CI components"),
    false,
    "docsite navigation lane must not install the Rust toolchain",
  );
  assertEquals(
    docsiteNavJob.includes("actions/cache/save"),
    false,
    "docsite navigation lane must not race canonical cache writers",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:docsite-nav"),
    ['{ task = "test:docsite:e2e:navigation" }'],
    "docsite navigation mise lane",
  );
  assertContainsAll(
    cp1FixturesJob,
    [
      "name: ci-cp1-fixtures",
      "needs: impact",
      "needs.impact.outputs.plan_cp1_acceptance == 'true'",
      "scripts/measure-step.sh cp1-fixtures mise run ci:cp1:fixtures",
      "name: ugoite-cp1-fixtures-query",
      "name: ugoite-cp1-fixtures-export",
      "name: ugoite-cp1-seeder",
      "target/cp1-fixtures/query",
      "target/cp1-fixtures/export",
      "target/cp1-fixtures/seeder",
      "target/cp1-profiling/",
    ],
    "CP1 fixtures producer lane",
  );
  assertEquals(
    cp1FixturesJob.includes("artifact-build"),
    false,
    "CP1 fixtures lane must not depend on the product build",
  );
  assertEquals(
    cp1FixturesJob.includes("ugoite-cli-linux"),
    false,
    "CP1 fixtures lane must not download product artifacts",
  );
  assertEquals(
    cp1FixturesJob.includes("ugoite-runtime-image"),
    false,
    "CP1 fixtures lane must not download product artifacts",
  );
  assertEquals(
    cp1FixturesJob.includes("cargo build -p ugoite-server"),
    false,
    "CP1 fixtures lane must not build the server",
  );
  assertEquals(
    cp1FixturesJob.includes("build:wasm"),
    false,
    "CP1 fixtures lane must not build the frontend",
  );
  assertContainsAll(
    cp1QueryJob,
    [
      "name: ci-cp1-query",
      "needs: [impact, artifact-build, cp1-fixtures]",
      "needs.impact.outputs.plan_cp1_acceptance == 'true'",
      "needs.artifact-build.result == 'success'",
      "needs.cp1-fixtures.result == 'success'",
      "UGOITE_ARTIFACT_SELECTION: runtime",
      "name: ugoite-artifact-manifest",
      "name: ugoite-runtime-image",
      "name: ugoite-cp1-fixtures-query",
      "name: ugoite-cp1-seeder",
      "target/cp1-fixtures/query",
      "target/cp1-seeder",
      "scripts/measure-step.sh load-artifacts mise run ci:artifacts:load",
      "scripts/measure-step.sh cp1-query mise run ci:lane:cp1-query",
      "scripts/measure-step.sh browser-deps deno task e2e:install:browsers",
      "BROWSER_DEPS_SECONDS",
      "target/cp1-profiling/",
      "target/query-surfaces-measurement.json",
    ],
    "CP1 query consumer lane",
  );
  assertEquals(
    cp1QueryJob.includes("name: ugoite-cli-linux"),
    false,
    "CP1 query lane must download only its fixture kind",
  );
  assertEquals(
    cp1QueryJob.includes("name: ugoite-cp1-fixtures-export"),
    false,
    "CP1 query lane must download only its fixture kind",
  );
  assertEquals(
    cp1QueryJob.includes("Install Rust CI components"),
    false,
    "CP1 query lane must not install the Rust toolchain",
  );
  assertEquals(
    cp1QueryJob.includes("cargo build"),
    false,
    "CP1 query lane must not build anything from source",
  );
  assertContainsAll(
    cp1ExportJob,
    [
      "name: ci-cp1-export",
      "needs: [impact, artifact-build, cp1-fixtures]",
      "needs.impact.outputs.plan_cp1_acceptance == 'true'",
      "needs.artifact-build.result == 'success'",
      "needs.cp1-fixtures.result == 'success'",
      "UGOITE_ARTIFACT_SELECTION: cli",
      "UGOITE_SQL_EXPORT_CLI_BINARY:",
      "name: ugoite-artifact-manifest",
      "name: ugoite-cli-linux",
      "name: ugoite-cp1-fixtures-export",
      "name: ugoite-cp1-seeder",
      "target/cp1-fixtures/export",
      "target/cp1-seeder",
      "downloaded_logical_bytes=",
      "bash scripts/measure-process-resources.sh target/cp1-profiling/sql-export-cli-load.time.txt mise run ci:artifacts:load",
      "scripts/measure-step.sh cp1-export mise run ci:lane:cp1-export",
      "target/cp1-profiling/",
      "target/cp1-profiling/sql-export-cli-transfer.json",
      "target/sql-export-measurement/",
    ],
    "CP1 export consumer lane",
  );
  assertEquals(
    cp1ExportJob.includes("name: ugoite-runtime-image"),
    false,
    "CP1 export lane must download only its fixture kind",
  );
  assertEquals(
    cp1ExportJob.includes("name: ugoite-cp1-fixtures-query"),
    false,
    "CP1 export lane must download only its fixture kind",
  );
  assertEquals(
    cp1ExportJob.includes("Install Rust CI components"),
    false,
    "CP1 export lane must not install the Rust toolchain",
  );
  assertEquals(
    cp1ExportJob.includes("cargo build"),
    false,
    "CP1 export lane must not build anything from source",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:cp1-acceptance"),
    [
      '{ task = "ci:cp1:prepare-fixtures" }',
      'UGOITE_CP1_FIXTURE_BUNDLE_DIR = "target/cp1-fixtures/query"',
      'UGOITE_CP1_FIXTURE_BUNDLE_DIR = "target/cp1-fixtures/export"',
    ],
    "CP1 acceptance mise lane",
  );
  assertContainsAll(
    taskBlock(mise, "ci:cp1:fixtures"),
    [
      '{ task = "ci:cp1:prepare-fixtures" }',
      "bash scripts/stage-cp1-seeder.sh",
    ],
    "CP1 fixtures mise entry",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:cp1-query"),
    [
      "bash scripts/assert-cp1-consumer-inputs.sh query",
      '{ task = "measure:query-surfaces" }',
      'UGOITE_QUERY_MEASURE_RUNNER = "compose"',
      'E2E_BUILD_IMAGES = "false"',
      'UGOITE_CP1_FIXTURE_BUNDLE_DIR = "target/cp1-fixtures/query"',
      'UGOITE_SEED_XTASK_BINARY = "target/cp1-seeder/xtask"',
    ],
    "CP1 query consumer mise lane",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:cp1-export"),
    [
      "bash scripts/assert-cp1-consumer-inputs.sh export",
      '{ task = "measure:sql-export" }',
      'UGOITE_CP1_FIXTURE_BUNDLE_DIR = "target/cp1-fixtures/export"',
      'UGOITE_SEED_XTASK_BINARY = "target/cp1-seeder/xtask"',
    ],
    "CP1 export consumer mise lane",
  );
  assertContainsAll(
    taskBlock(mise, "ci:cp1:prepare-fixtures"),
    [
      '{ task = "ci:cp1:build-seeder" }',
      "bash scripts/prepare-cp1-fixtures.sh",
    ],
    "CP1 fixture producer task",
  );
  assertContainsAll(
    taskBlock(mise, "ci:cp1:build-seeder"),
    [
      'CARGO_TARGET_DIR = "target/rust"',
      "target/cp1-profiling/xtask-build.time.txt",
      "bash scripts/build-cp1-seeder.sh",
    ],
    "single measured CP1 seeder build",
  );
  assertContainsAll(
    querySurfaceMeasure,
    [
      'tools/cp1_fixture_spec.ts" query',
      'QUERY_PROFILE_ARGS+=(--seed "$fixture_slug"',
      '--scenario "${QUERY_FIXTURE_SCENARIOS[$index]}"',
      '--entry-count "${QUERY_FIXTURE_COUNTS[$index]}"',
    ],
    "query measurement shared fixture specification",
  );
  assertContainsAll(
    sqlExportMeasure,
    [
      'tools/cp1_fixture_spec.ts" export',
      '--space-id "$FIXTURE_SLUG"',
      '--entry-count "$FIXTURE_ENTRY_COUNT"',
      '--seed "$FIXTURE_SEED"',
    ],
    "SQL export shared fixture specification",
  );
  assertEquals(
    [
      "3134001",
      "3134002",
      "3140001",
      "6000",
      "4000",
      "10000",
      "Query Measurement Owner",
    ].some((value) =>
      querySurfaceMeasure.includes(value) || sqlExportMeasure.includes(value)
    ),
    false,
    "CP1 measurement scripts must not duplicate shared fixture values",
  );
  assertContainsAll(
    querySurfaceMeasure,
    [
      "QUERY_EXPECTED_JSON=",
      'UGOITE_QUERY_MEASURE_EXPECTED_JSON="$QUERY_EXPECTED_JSON"',
    ],
    "query measurement expected-count handoff",
  );
  assertContainsAll(
    queryBrowserTest,
    [
      "UGOITE_QUERY_MEASURE_EXPECTED_JSON",
      "expectedFixtureFor",
    ],
    "browser query assertions derive counts from the shared fixture spec",
  );
  assertEquals(
    ["6_000", "4_000", "6,000", "4,000"].some((value) =>
      queryBrowserTest.includes(value)
    ),
    false,
    "browser query assertions must not hardcode fixture entry counts",
  );
  assertContainsAll(
    sqlExportMeasure,
    [
      '--max-rows "$FIXTURE_ENTRY_COUNT"',
      "--max-bytes 1024",
      "max-bytes would be exceeded",
      "A byte-limited export must not publish a partial destination",
      "A failed byte-limited export left a temporary output file",
    ],
    "fixed SQL export acceptance fixture",
  );
  assertContainsAll(
    taskBlock(mise, "ci:impact"),
    ["deno run -A tools/ci-impact.ts"],
    "standalone CI impact mise task",
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
      "needs: [impact, rust-check, rust-test, s3-shared-authorization, web, artifact-build, e2e-smoke-mobile, e2e-owner, e2e-portable, docsite-nav, cp1-fixtures, cp1-query, cp1-export, pr-context-report]",
      "runs-on: ubuntu-slim",
      "IMPACT_RESULT: ${{ needs.impact.result }}",
      "IMPACT_PLAN_STATUS: ${{ needs.impact.outputs.plan_status }}",
      "IMPACT_PLAN_SCOPE: ${{ needs.impact.outputs.plan_scope }}",
      "IMPACT_JOBS_SKIPPED: ${{ needs.impact.outputs.jobs_skipped }}",
      "PLAN_RUST_CHECK: ${{ needs.impact.outputs.plan_rust_check }}",
      "PLAN_RUST_TEST: ${{ needs.impact.outputs.plan_rust_test }}",
      "PLAN_WEB: ${{ needs.impact.outputs.plan_web }}",
      "PLAN_ARTIFACTS: ${{ needs.impact.outputs.plan_artifacts }}",
      "PLAN_DOCSITE_NAV: ${{ needs.impact.outputs.plan_docsite_nav }}",
      "PLAN_CP1_ACCEPTANCE: ${{ needs.impact.outputs.plan_cp1_acceptance }}",
      "RUST_CHECK_RESULT: ${{ needs.rust-check.result }}",
      "RUST_TEST_RESULT: ${{ needs.rust-test.result }}",
      "S3_SHARED_AUTHORIZATION_RESULT: ${{ needs.s3-shared-authorization.result }}",
      "WEB_RESULT: ${{ needs.web.result }}",
      "ARTIFACT_BUILD_RESULT: ${{ needs.artifact-build.result }}",
      "E2E_SMOKE_MOBILE_RESULT: ${{ needs.e2e-smoke-mobile.result }}",
      "E2E_OWNER_RESULT: ${{ needs.e2e-owner.result }}",
      "E2E_PORTABLE_RESULT: ${{ needs.e2e-portable.result }}",
      "DOCSITE_NAV_RESULT: ${{ needs.docsite-nav.result }}",
      "CP1_FIXTURES_RESULT: ${{ needs.cp1-fixtures.result }}",
      "CP1_QUERY_RESULT: ${{ needs.cp1-query.result }}",
      "CP1_EXPORT_RESULT: ${{ needs.cp1-export.result }}",
      "PR_CONTEXT_RESULT: ${{ needs.pr-context-report.result }}",
      "run: scripts/ci-gate-check.sh",
    ],
    "required CI aggregator",
  );
  assertContainsAll(
    requiredJob,
    [
      "actions/checkout@",
      "sparse-checkout: scripts/ci-gate-check.sh",
      "sparse-checkout-cone-mode: false",
    ],
    "minimal required CI checkout",
  );

  assertMainTrigger(workflow, "pull_request");
  assertMainTrigger(workflow, "merge_group");
  assertMainTrigger(workflow, "push");
}

Deno.test("CI aggregate tasks own test coverage and lane scheduling", async () => {
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");

  await assertAggregateWorkflow(workflow, mise);
});

Deno.test("required CI workflows cover base-ref edits without cancelling content validation", async () => {
  const ci = await Deno.readTextFile(".github/workflows/ci.yml");
  const codeql = await Deno.readTextFile(".github/workflows/codeql.yml");
  const impactJob = workflowJobBlock(ci, "impact");
  const prContextJob = workflowJobBlock(ci, "pr-context-report");
  const codeqlAnalyzeJob = workflowJobBlock(codeql, "analyze");
  const codeqlRequiredJob = workflowJobBlock(codeql, "required-check");
  const pullRequestActivities = [
    "pull_request:\n    branches:\n      - main\n    types:\n      - opened\n      - synchronize\n      - reopened\n      - ready_for_review\n      - edited",
  ];

  assertContainsAll(ci, pullRequestActivities, "CI pull request trigger");
  assertContainsAll(
    codeql,
    pullRequestActivities,
    "CodeQL pull request trigger",
  );
  assertContainsAll(
    ci,
    [
      "github.event.action == 'edited' && github.event.changes.base == null && 'metadata' || 'validation'",
      "EVENT_ACTION: ${{ github.event.action }}",
      "PR_BASE_CHANGED: ${{ github.event.changes.base != null }}",
    ],
    "CI event isolation and gate context",
  );
  assertContainsAll(
    impactJob,
    [
      "github.event_name != 'pull_request' ||\n      github.event.action != 'edited' ||\n      github.event.changes.base != null",
    ],
    "CI impact planner event guard",
  );
  assertContainsAll(
    prContextJob,
    [
      "github.event_name == 'pull_request' &&\n      (github.event.action != 'edited' || github.event.changes.base != null)",
    ],
    "CI context report event guard",
  );
  assertContainsAll(
    codeqlAnalyzeJob,
    [
      "github.event_name != 'pull_request' ||\n      github.event.action != 'edited' ||\n      github.event.changes.base != null",
    ],
    "CodeQL analysis event guard",
  );
  assertContainsAll(
    codeqlRequiredJob,
    [
      "if: ${{ always() }}",
      "needs:\n      - analyze",
      'select(.value.result != "success" and .value.result != "skipped")',
    ],
    "CodeQL required summary for skipped metadata analysis",
  );

  assertEquals(
    await ciGatePasses({
      PR_BASE_CHANGED: "false",
      IMPACT_RESULT: "skipped",
      IMPACT_PLAN_STATUS: "",
      IMPACT_PLAN_SCOPE: "",
      IMPACT_JOBS_SKIPPED: "",
      RUST_CHECK_RESULT: "skipped",
      RUST_TEST_RESULT: "skipped",
      S3_SHARED_AUTHORIZATION_RESULT: "skipped",
      WEB_RESULT: "skipped",
      ARTIFACT_BUILD_RESULT: "skipped",
      E2E_SMOKE_MOBILE_RESULT: "skipped",
      E2E_OWNER_RESULT: "skipped",
      E2E_PORTABLE_RESULT: "skipped",
      DOCSITE_NAV_RESULT: "skipped",
      CP1_FIXTURES_RESULT: "skipped",
      CP1_QUERY_RESULT: "skipped",
      CP1_EXPORT_RESULT: "skipped",
      PR_CONTEXT_RESULT: "skipped",
    }),
    true,
    "metadata-only edits should pass with successful aggregate checks",
  );
  assertEquals(
    await ciGatePasses({ PR_BASE_CHANGED: "true" }),
    true,
    "base-ref edits should use the ordinary validation results",
  );
  assertEquals(
    await ciGatePasses({ PR_BASE_CHANGED: "false", WEB_RESULT: "success" }),
    false,
    "metadata-only edits should reject a content lane that unexpectedly ran",
  );
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

Deno.test("REQ-OPS-024: docsite coverage remains developer convenience", async () => {
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
    ],
    "docsite coverage config",
  );
  assertEquals(
    docsiteConfig.includes("thresholds:"),
    false,
    "docsite coverage config must not hard-gate thresholds",
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
    taskBlock(mise, "test:docsite"),
    ["deno task docsite:test"],
    "docsite root test task",
  );
  assertContainsAll(
    taskBlock(mise, "test"),
    ["test:docsite"],
    "canonical test task",
  );
  assertEquals(
    taskBlock(mise, "test").includes("test:docsite:coverage"),
    false,
    "canonical test task must leave docsite coverage as developer convenience",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:web"),
    ['"test:docsite"'],
    "hosted web lane",
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

function composeRunnerHelper(source: string): string {
  const start = source.indexOf("run_verified_portable_cli() {");
  assertEquals(start >= 0, true, "post-claim CLI helper must exist");
  const end = source.indexOf("\n}", start);
  assertEquals(end >= 0, true, "post-claim CLI helper must terminate");
  return source.slice(start, end + "\n}".length);
}

Deno.test("portable post-claim verification reuses the verified CLI", async () => {
  const composeRunner = await Deno.readTextFile(
    new URL("../e2e/scripts/run-e2e-compose.sh", import.meta.url),
  );
  assertContainsAll(
    composeRunner,
    [
      "run_verified_portable_cli",
      'run_verified_portable_cli "$PORTABLE_CLI_CONFIG" space verify --deep --format json',
      'local binary="${UGOITE_PORTABLE_CLI_BINARY:-}"',
      '[ ! -x "$binary" ]',
      '"${binary}.source-sha"',
      '"$(cat "$source_sha_file")" != "$CHECKOUT_SOURCE_SHA"',
      "cargo run -q --manifest-path",
    ],
    "portable post-claim CLI selection and provenance checks",
  );
  assertEquals(
    (composeRunner.match(/cargo run -q --manifest-path/g) ?? []).length,
    1,
    "post-claim verification uses the selected CLI runner instead of recompiling",
  );
});

Deno.test("verified post-claim CLI fails closed on provenance mismatch", async () => {
  const composeRunner = await Deno.readTextFile(
    new URL("../e2e/scripts/run-e2e-compose.sh", import.meta.url),
  );
  const helper = composeRunnerHelper(composeRunner);
  const root = await Deno.makeTempDir({ prefix: "ugoite-post-claim-cli-" });
  try {
    const binDir = `${root}/rel`;
    await Deno.mkdir(binDir, { recursive: true });
    const fakeBinary = `${binDir}/ugoite`;
    await Deno.writeTextFile(
      fakeBinary,
      "#!/bin/sh\nprintf 'fake-cli %s\\n' \"$@\"\n",
    );
    await Deno.chmod(fakeBinary, 0o755);
    const checkoutSha = "0".repeat(40);
    await Deno.writeTextFile(`${fakeBinary}.source-sha`, `${checkoutSha}\n`);

    const runHelper = async (
      cliBinary: string | null,
      sidecar: string,
    ): Promise<{ success: boolean; stdout: string; stderr: string }> => {
      await Deno.writeTextFile(`${fakeBinary}.source-sha`, sidecar);
      const harness =
        `${helper}\nrun_verified_portable_cli "$UGOITE_PORTABLE_CLI_CONFIG" space verify --deep --format json\n`;
      const env: Record<string, string> = {
        ...Deno.env.toObject(),
        ROOT_DIR: root,
        CHECKOUT_SOURCE_SHA: checkoutSha,
        UGOITE_PORTABLE_CLI_CONFIG: `${root}/cli-config.toml`,
      };
      if (cliBinary !== null) env.UGOITE_PORTABLE_CLI_BINARY = cliBinary;
      else delete env.UGOITE_PORTABLE_CLI_BINARY;
      const output = await new Deno.Command("bash", {
        args: ["-c", harness],
        env,
        stdout: "piped",
        stderr: "piped",
      }).output();
      const decoder = new TextDecoder();
      return {
        success: output.success,
        stdout: decoder.decode(output.stdout),
        stderr: decoder.decode(output.stderr),
      };
    };

    const verified = await runHelper("rel/ugoite", `${checkoutSha}\n`);
    assertEquals(verified.success, true, verified.stderr);
    assertContainsAll(
      verified.stdout,
      [
        "fake-cli --config",
        "fake-cli space",
        "fake-cli verify",
        "fake-cli --deep",
        "fake-cli json",
      ],
      "verified CLI receives the post-claim verify arguments",
    );

    const mismatched = await runHelper("rel/ugoite", "f".repeat(40) + "\n");
    assertEquals(mismatched.success, false);
    assertEquals(
      mismatched.stderr.includes("source SHA does not match checkout"),
      true,
      mismatched.stderr,
    );

    const missing = await runHelper("rel/missing-ugoite", `${checkoutSha}\n`);
    assertEquals(missing.success, false);
    assertEquals(
      missing.stderr.includes("is not executable"),
      true,
      missing.stderr,
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});
