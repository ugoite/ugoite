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

Deno.test("root quality entrypoints retain required validation suites", async () => {
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");
  assertContainsAll(
    taskBlock(mise, "ci"),
    [
      '{ task = "fmt:check" }',
      '{ task = "lint" }',
      '{ task = "check" }',
      '{ task = "test" }',
    ],
    "root CI entrypoint",
  );
  assertContainsAll(
    workflow,
    ["run: scripts/ci-gate-check.sh"],
    "required hosted quality gate entrypoints",
  );
});

Deno.test("required CI aggregator waits for every required lane", async () => {
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");
  assertEquals(
    /needs:\s*\[\s*impact,\s*rust-check,\s*rust-test,\s*s3-shared-authorization,\s*web,\s*artifact-build,\s*e2e-smoke-mobile,\s*e2e-composition-studio,\s*e2e-owner-composition,\s*e2e-portable,\s*docsite-nav,\s*cp1-fixtures,\s*cp1-query,\s*cp1-export,\s*pr-context-report,?\s*\]/
      .test(workflow),
    true,
    "required CI aggregator must wait for every required lane",
  );
  const requiredStart = workflow.indexOf("\n  required:");
  assertEquals(requiredStart >= 0, true, "required CI job must exist");
  assertContainsAll(
    workflow.slice(requiredStart),
    [
      "if: ${{ always() }}",
      "IMPACT_RESULT: ${{ needs.impact.result }}",
      "IMPACT_PLAN_STATUS: ${{ needs.impact.outputs.plan_status }}",
      "PLAN_RUST_CHECK: ${{ needs.impact.outputs.plan_rust_check }}",
      "RUST_CHECK_RESULT: ${{ needs.rust-check.result }}",
      "run: scripts/ci-gate-check.sh",
    ],
    "required CI job invokes the fail-closed gate with CI and plan results",
  );
});

Deno.test("Rust quality gate retains workspace checks and tests", async () => {
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");
  assertContainsAll(
    taskBlock(mise, "check"),
    ['{ task = "check:rust" }'],
    "root check entrypoint",
  );
  assertContainsAll(
    taskBlock(mise, "test"),
    ['"test:rust"'],
    "root test entrypoint",
  );
  assertContainsAll(
    taskBlock(mise, "ci"),
    ['{ task = "check" }', '{ task = "test" }'],
    "root quality entrypoint",
  );
  assertContainsAll(
    taskBlock(mise, "check:rust"),
    ["cargo check --workspace --all-targets --all-features --locked"],
    "Rust workspace check",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:rust-check"),
    ['{ task = "check:rust:wasm" }'],
    "hosted Rust check entrypoint",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:rust-test"),
    ['{ task = "test:rust:core" }'],
    "hosted Rust test entrypoint",
  );
  assertContainsAll(
    workflow,
    ["mise run ci:lane:rust-check", "mise run ci:lane:rust-test"],
    "hosted Rust lanes",
  );
});

Deno.test("docsite suite remains in root and hosted web validation", async () => {
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");
  assertContainsAll(
    taskBlock(mise, "test"),
    ['"test:docsite"'],
    "root test entrypoint",
  );
  assertContainsAll(
    taskBlock(mise, "test:docsite"),
    ["deno task docsite:test"],
    "normal docsite behavior/type suite",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:web"),
    ['"test:docsite"'],
    "hosted web lane",
  );
  assertContainsAll(
    workflow,
    ["mise run ci:lane:web"],
    "hosted web workflow entrypoint",
  );
});

Deno.test("portable WASM crates remain in the Rust check gate", async () => {
  const mise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");
  assertContainsAll(
    taskBlock(mise, "check"),
    ['{ task = "check:rust" }'],
    "root check entrypoint",
  );
  assertContainsAll(
    taskBlock(mise, "check:rust"),
    ['{ task = "check:rust:wasm" }'],
    "root Rust check entrypoint",
  );
  assertContainsAll(
    taskBlock(mise, "check:rust:wasm"),
    [
      "cargo check -p ugoite-domain --target wasm32-unknown-unknown --locked",
      "cargo check -p ugoite-api-client --target wasm32-unknown-unknown --locked",
      "cargo check -p ugoite-wasm --target wasm32-unknown-unknown --locked",
    ],
    "portable WASM compile checks",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:rust-check"),
    ['{ task = "check:rust:wasm" }'],
    "hosted Rust check entrypoint",
  );
  assertContainsAll(
    workflow,
    ["mise run ci:lane:rust-check"],
    "hosted Rust check lane",
  );
});

Deno.test("base-ref edits remain in required validation workflows", async () => {
  const ci = await Deno.readTextFile(".github/workflows/ci.yml");
  const codeql = await Deno.readTextFile(".github/workflows/codeql.yml");

  const pullRequestEditedEvent = [
    "pull_request:\n    branches:\n      - main\n    types:",
    "      - edited",
  ];
  assertContainsAll(ci, pullRequestEditedEvent, "CI pull request trigger");
  assertContainsAll(
    codeql,
    pullRequestEditedEvent,
    "CodeQL pull request trigger",
  );

  // A title/body-only edit may take the metadata path, but a changed base ref
  // must schedule the normal planner, PR context, and CodeQL analysis.
  assertContainsAll(
    ci,
    [
      "github.event_name == 'pull_request' && github.event.action == 'edited' && github.event.changes.base == null && 'metadata' || 'validation'",
      "github.event_name != 'pull_request' ||\n      github.event.action != 'edited' ||\n      github.event.changes.base != null",
      "github.event_name == 'pull_request' &&\n      (github.event.action != 'edited' || github.event.changes.base != null)",
      "PR_BASE_CHANGED: ${{ github.event.changes.base != null }}",
    ],
    "CI base-ref and metadata event routing",
  );
  assertContainsAll(
    codeql,
    [
      "github.event_name != 'pull_request' ||\n      github.event.action != 'edited' ||\n      github.event.changes.base != null",
      "if: ${{ always() }}",
      'select(.value.result != "success" and .value.result != "skipped")',
    ],
    "CodeQL base-ref and required summary",
  );
});

Deno.test("REQ-OPS-021: frontend coverage remains a required hosted test gate", async () => {
  const frontendConfig = await Deno.readTextFile("frontend/vitest.config.ts");
  const rootDeno = await Deno.readTextFile("deno.json");
  const mise = await Deno.readTextFile("mise.toml");
  assertEquals(
    frontendTasks.tasks.coverage,
    "deno run -A vitest run --coverage --maxWorkers=1",
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
    ['"test:frontend"'],
    "canonical test task keeps ordinary frontend behavior tests",
  );
  assertContainsAll(
    taskBlock(mise, "test:low-memory"),
    ['"test:frontend"'],
    "low-memory test task keeps ordinary frontend behavior tests",
  );
  assertEquals(
    taskBlock(mise, "test").includes("test:frontend:coverage"),
    false,
    "canonical test task must leave frontend coverage to the hosted web gate",
  );
  assertEquals(
    taskBlock(mise, "test:low-memory").includes("test:frontend:coverage"),
    false,
    "low-memory test task must leave frontend coverage to the hosted web gate",
  );
  assertContainsAll(
    taskBlock(mise, "ci:lane:web"),
    ['"test:frontend:coverage"'],
    "required hosted web lane",
  );
});

Deno.test("REQ-OPS-024: docsite coverage remains developer convenience", async () => {
  const docsiteConfig = await Deno.readTextFile("docsite/vitest.config.ts");
  const rootDeno = await Deno.readTextFile("deno.json");
  const mise = await Deno.readTextFile("mise.toml");
  assertEquals(
    docsiteTasks.tasks.coverage,
    "deno run -A vitest run --coverage --maxWorkers=1",
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
  assertEquals(
    taskBlock(mise, "test").includes("test:docsite:coverage"),
    false,
    "canonical test task must leave docsite coverage as developer convenience",
  );
});

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

function composeRunnerHelper(source: string): string {
  const start = source.indexOf("run_verified_portable_cli() {");
  assertEquals(start >= 0, true, "post-claim CLI helper must exist");
  const end = source.indexOf("\n}", start);
  assertEquals(end >= 0, true, "post-claim CLI helper must terminate");
  return source.slice(start, end + "\n}".length);
}

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
