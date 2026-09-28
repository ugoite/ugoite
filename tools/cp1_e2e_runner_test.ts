import { assertEquals } from "@std/assert/equals";

const directRunner = await Deno.readTextFile(
  new URL("../e2e/scripts/run-e2e.sh", import.meta.url),
);
const composeRunner = await Deno.readTextFile(
  new URL("../e2e/scripts/run-e2e-compose.sh", import.meta.url),
);
const taskHelperPath = await Deno.realPath(
  new URL("../e2e/scripts/run-e2e-task.sh", import.meta.url),
);
const taskHelper = await Deno.readTextFile(taskHelperPath);
const queryMeasure = await Deno.readTextFile(
  new URL("../scripts/measure-query-surfaces.sh", import.meta.url),
);

Deno.test("direct and Compose E2E runners share the task and JUnit gate", async () => {
  assertEquals(
    directRunner.includes('source "$SCRIPT_DIR/run-e2e-task.sh"'),
    true,
  );
  assertEquals(
    composeRunner.includes('source "$SCRIPT_DIR/run-e2e-task.sh"'),
    true,
  );
  assertEquals(
    directRunner.includes(
      'run_e2e_task query-measurement "$base_report_file" "$ENFORCE_CI_GATES"',
    ),
    true,
  );
  assertEquals(
    composeRunner.includes(
      'run_e2e_task query-measurement "$base_report_file" true',
    ),
    true,
  );
  assertEquals(taskHelper.includes("zero executed tests"), true);
  assertEquals(taskHelper.includes("skipped=${skipped} is not allowed"), true);

  const report = await Deno.makeTempFile({ suffix: ".xml" });
  try {
    const runGate = async (tests: number, skipped: number) => {
      await Deno.writeTextFile(
        report,
        `<testsuites><testsuite tests="${tests}" skipped="${skipped}"/></testsuites>`,
      );
      return await new Deno.Command("bash", {
        args: [
          "-c",
          'source "$1"; validate_junit_report "$2"',
          "test-junit-gate",
          taskHelperPath,
          report,
        ],
        stdout: "piped",
        stderr: "piped",
      }).output();
    };

    assertEquals((await runGate(1, 0)).success, true);
    assertEquals((await runGate(0, 0)).success, false);
    assertEquals((await runGate(1, 1)).success, false);
  } finally {
    await Deno.remove(report);
  }
});

Deno.test("Compose query fixtures retain caller ownership and private modes", () => {
  assertEquals(composeRunner.includes("--fixture-root"), true);
  assertEquals(composeRunner.includes("STORAGE_ROOT_OWNED=false"), true);
  assertEquals(composeRunner.includes("STORAGE_ROOT_OWNED=true"), true);
  assertEquals(
    composeRunner.includes('if [ "$STORAGE_ROOT_OWNED" = true ]'),
    true,
  );
  assertEquals(composeRunner.includes("chmod 0777"), false);
  assertEquals(composeRunner.includes("chown -R ugoite:ugoite /data"), true);
  assertEquals(
    composeRunner.includes("chown -R $(id -u):$(id -g) /data"),
    true,
  );
  assertEquals(
    composeRunner.includes('PORTABLE_PROOF_HOST_FILE="$(mktemp'),
    true,
  );
  assertEquals(
    composeRunner.includes(
      'export E2E_PORTABLE_PROOF_FILE="$PORTABLE_PROOF_HOST_FILE"',
    ),
    true,
  );
  assertEquals(
    composeRunner.includes('rm -f "$PORTABLE_PROOF_HOST_FILE"'),
    true,
  );
});

Deno.test("Compose query runner leaves a caller-owned fixture root in place", async () => {
  const tempRoot = await Deno.makeTempDir({
    prefix: "ugoite-compose-runner-test-",
  });
  const fixtureRoot = `${tempRoot}/fixture`;
  const bin = `${tempRoot}/bin`;
  const dockerLog = `${tempRoot}/docker.log`;
  const report = `${tempRoot}/junit.xml`;
  await Deno.mkdir(`${fixtureRoot}/spaces/mock-space`, { recursive: true });
  await Deno.mkdir(bin);
  await Deno.writeTextFile(
    `${fixtureRoot}/spaces/mock-space/meta.json`,
    "{}\n",
  );
  const initialMode = (await Deno.stat(fixtureRoot)).mode! & 0o777;

  const dockerStub = `${bin}/docker`;
  await Deno.writeTextFile(
    dockerStub,
    `#!/usr/bin/env bash
set -e
printf '%s\\n' "$*" >>"$DOCKER_LOG"
case " $* " in
  *" port ugoite 8000 "*) printf '127.0.0.1:18081\\n' ;;
  *" logs "*) printf 'ugoite server #secret=fixture-secret\\n' ;;
esac
`,
  );
  await Deno.chmod(dockerStub, 0o755);

  const curlStub = `${bin}/curl`;
  await Deno.writeTextFile(
    curlStub,
    `#!/usr/bin/env bash
set -e
args="$*"
url=""
for arg in "$@"; do url="$arg"; done
if [[ "$args" == *"-w"* ]]; then printf '200'; exit 0; fi
case "$url" in
  */health)
    if [[ "$args" == *"-sSI"* ]]; then
      printf 'HTTP/1.1 200 OK\\r\\nX-Ugoite-Source-Sha: %s\\r\\n\\r\\n' "$TEST_SOURCE_SHA"
    fi
    ;;
  */build-info.json) printf '{"source_sha":"%s"}\\n' "$TEST_SOURCE_SHA" ;;
  */setup) printf '<div id="app"></div><script src="/_build/client.js"></script>' ;;
esac
`,
  );
  await Deno.chmod(curlStub, 0o755);

  const denoStub = `${bin}/deno`;
  await Deno.writeTextFile(
    denoStub,
    `#!/usr/bin/env bash
set -e
if [[ "$1" == "eval" && "$2" == *"Deno.listen"* ]]; then printf '18080\\n'; exit 0; fi
if [[ "$1" == "task" ]]; then
  printf '<testsuites><testsuite tests="1" skipped="0"></testsuite></testsuites>\\n' >"$PLAYWRIGHT_JUNIT_OUTPUT_FILE"
  exit 0
fi
exec "$REAL_DENO" "$@"
`,
  );
  await Deno.chmod(denoStub, 0o755);

  const git = await new Deno.Command("git", {
    args: ["rev-parse", "HEAD"],
  }).output();
  const sourceSha = new TextDecoder().decode(git.stdout).trim();
  try {
    const result = await new Deno.Command("bash", {
      args: [
        "e2e/scripts/run-e2e-compose.sh",
        "query-measurement",
        "--fixture-root",
        fixtureRoot,
      ],
      env: {
        PATH: `${bin}:${Deno.env.get("PATH") ?? ""}`,
        REAL_DENO: Deno.execPath(),
        DOCKER_LOG: dockerLog,
        TEST_SOURCE_SHA: sourceSha,
        UGOITE_SOURCE_SHA: sourceSha,
        UGOITE_SKIP_PLAYWRIGHT_DEPS: "1",
        E2E_BUILD_IMAGES: "false",
        E2E_READINESS_TIMEOUT_SECONDS: "2",
        E2E_BACKEND_START_TIMEOUT_SECONDS: "2",
        E2E_OIDC_MOCK_HOST: "192.0.2.2",
        PLAYWRIGHT_JUNIT_OUTPUT_FILE: report,
      },
      stdout: "piped",
      stderr: "piped",
    }).output();
    assertEquals(
      result.success,
      true,
      `${new TextDecoder().decode(result.stdout)}\n${
        new TextDecoder().decode(result.stderr)
      }\nDocker calls:\n${await Deno.readTextFile(dockerLog)}`,
    );
    const dockerCalls = await Deno.readTextFile(dockerLog);
    assertEquals(dockerCalls.includes(`${fixtureRoot}:/data`), true);
    assertEquals(dockerCalls.includes(" down -v"), true);
    assertEquals(
      dockerCalls.includes("chown -R ugoite:ugoite /data"),
      true,
    );
    assertEquals(/chown -R \d+:\d+ \/data/.test(dockerCalls), true);
    assertEquals((await Deno.stat(fixtureRoot)).isDirectory, true);
    assertEquals(
      (await Deno.stat(`${fixtureRoot}/spaces/mock-space/meta.json`)).isFile,
      true,
    );
    assertEquals((await Deno.stat(fixtureRoot)).mode! & 0o777, initialMode);
  } finally {
    await Deno.remove(tempRoot, { recursive: true });
  }
});

Deno.test("Compose query refuses to start without an explicit fixture root", async () => {
  const result = await new Deno.Command("bash", {
    args: ["e2e/scripts/run-e2e-compose.sh", "query-measurement"],
    stdout: "piped",
    stderr: "piped",
  }).output();
  assertEquals(result.code, 1);
  assertEquals(
    new TextDecoder().decode(result.stderr).includes(
      "query-measurement requires --fixture-root",
    ),
    true,
  );
});

Deno.test("query measurement keeps host default and exposes Compose parity mode", () => {
  assertEquals(
    queryMeasure.includes("UGOITE_QUERY_MEASURE_RUNNER:-host"),
    true,
  );
  assertEquals(queryMeasure.includes("query-measurement --fixture-root"), true);
  assertEquals(
    queryMeasure.includes('QUERY_E2E_PROFILE_STEP="query-playwright-compose"'),
    true,
  );
});
