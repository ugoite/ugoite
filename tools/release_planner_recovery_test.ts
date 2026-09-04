import { assertEquals } from "@std/assert/equals";

const scriptPath = new URL(
  "../scripts/recover-release-planner-ref.sh",
  import.meta.url,
).pathname;
const expectedSha = "a".repeat(40);
const changedSha = "b".repeat(40);
const expectedReadPath =
  "api repos/ugoite/ugoite/git/ref/heads/release-planner-orphan --jq .object.sha";
const expectedDeletePath =
  "api --method DELETE repos/ugoite/ugoite/git/refs/heads/release-planner-orphan";

type Harness = {
  root: string;
  env: Record<string, string>;
  logPath: string;
};

async function createHarness(options: {
  initialSha: string;
  finalSha?: string;
}): Promise<Harness> {
  const root = await Deno.makeTempDir({ prefix: "ugoite-release-recovery-" });
  const fakeBin = `${root}/bin`;
  const logPath = `${root}/gh.log`;
  const statePath = `${root}/gh.state`;
  await Deno.mkdir(fakeBin, { recursive: true });
  await Deno.writeTextFile(
    `${fakeBin}/gh`,
    `#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "\$*" >> "\${RECOVERY_GH_LOG}"
if [[ "\${1:-}" != api ]]; then exit 1; fi
if [[ "\${2:-}" == --method && "\${3:-}" == DELETE ]]; then exit 0; fi
if [[ ! -s "\${RECOVERY_GH_STATE}" ]]; then
  printf 'read\\n' > "\${RECOVERY_GH_STATE}"
  printf '%s\\n' "\${RECOVERY_INITIAL_SHA}"
else
  printf '%s\\n' "\${RECOVERY_FINAL_SHA}"
fi
`,
  );
  await Deno.chmod(`${fakeBin}/gh`, 0o755);
  return {
    root,
    logPath,
    env: {
      PATH: `${fakeBin}:${Deno.env.get("PATH") ?? ""}`,
      RECOVERY_GH_LOG: logPath,
      RECOVERY_GH_STATE: statePath,
      RECOVERY_INITIAL_SHA: options.initialSha,
      RECOVERY_FINAL_SHA: options.finalSha ?? options.initialSha,
    },
  };
}

async function runRecovery(
  harness: Harness,
  args: string[],
): Promise<Deno.CommandOutput> {
  return await new Deno.Command("bash", {
    args: [scriptPath, ...args],
    cwd: harness.root,
    env: harness.env,
    stdout: "piped",
    stderr: "piped",
  }).output();
}

async function withHarness(
  options: Parameters<typeof createHarness>[0],
  callback: (harness: Harness) => Promise<void>,
): Promise<void> {
  const harness = await createHarness(options);
  try {
    await callback(harness);
  } finally {
    await Deno.remove(harness.root, { recursive: true });
  }
}

const recoveryArgs = [
  "--repo",
  "ugoite/ugoite",
  "--ref",
  "refs/heads/release-planner-orphan",
  "--expected-sha",
  expectedSha,
];

Deno.test("release planner recovery is a read-only dry run by default", async () => {
  await withHarness({ initialSha: expectedSha }, async (harness) => {
    const result = await runRecovery(harness, recoveryArgs);
    const stdout = new TextDecoder().decode(result.stdout);
    const log = await Deno.readTextFile(harness.logPath);
    assertEquals(result.success, true);
    assertEquals(stdout.includes("Recovery target:"), true);
    assertEquals(stdout.includes("Dry run: no mutation performed"), true);
    assertEquals(log.trim().split("\n"), [expectedReadPath]);
  });
});

Deno.test("release planner recovery deletes only after a stable final comparison", async () => {
  await withHarness({ initialSha: expectedSha }, async (harness) => {
    const result = await runRecovery(harness, [...recoveryArgs, "--delete"]);
    const stdout = new TextDecoder().decode(result.stdout);
    const log = await Deno.readTextFile(harness.logPath);
    assertEquals(result.success, true);
    assertEquals(stdout.includes("Deleted branch ref:"), true);
    assertEquals(log.trim().split("\n"), [
      expectedReadPath,
      expectedReadPath,
      expectedDeletePath,
    ]);
  });
});

Deno.test("release planner recovery aborts when the ref changes before deletion", async () => {
  await withHarness(
    { initialSha: expectedSha, finalSha: changedSha },
    async (harness) => {
      const result = await runRecovery(harness, [...recoveryArgs, "--delete"]);
      const stderr = new TextDecoder().decode(result.stderr);
      const log = await Deno.readTextFile(harness.logPath);
      assertEquals(result.success, false);
      assertEquals(stderr.includes("Ref changed during recovery check"), true);
      assertEquals(log.trim().split("\n"), [
        expectedReadPath,
        expectedReadPath,
      ]);
    },
  );
});

Deno.test("release planner recovery rejects tag refs before contacting GitHub", async () => {
  await withHarness({ initialSha: expectedSha }, async (harness) => {
    const result = await runRecovery(harness, [
      "--repo",
      "ugoite/ugoite",
      "--ref",
      "refs/tags/v0.1.0",
      "--expected-sha",
      expectedSha,
    ]);
    const stderr = new TextDecoder().decode(result.stderr);
    assertEquals(result.success, false);
    assertEquals(stderr.includes("refs/heads"), true);
    try {
      await Deno.stat(harness.logPath);
      throw new Error("GitHub must not be contacted for a tag ref");
    } catch (error) {
      if (!(error instanceof Deno.errors.NotFound)) throw error;
    }
  });
});
