import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);
const scriptPath = new URL("../scripts/mitase", import.meta.url).pathname;

async function readText(path: string): Promise<string> {
  return await Deno.readTextFile(new URL(path, root));
}

async function executable(path: string, contents: string): Promise<void> {
  await Deno.writeTextFile(path, contents);
  await Deno.chmod(path, 0o755);
}

async function createHarness(): Promise<{
  root: string;
  archive: string;
  marker: string;
  env: Record<string, string>;
}> {
  const harnessRoot = await Deno.makeTempDir({ prefix: "ugoite-mitase-tool-" });
  const fakeBin = `${harnessRoot}/bin`;
  const fixtureDir = `${harnessRoot}/fixture`;
  const archive =
    `${harnessRoot}/mitase-v0.1.3-x86_64-unknown-linux-gnu.tar.gz`;
  const marker = `${harnessRoot}/invocation.txt`;
  await Deno.mkdir(fakeBin, { recursive: true });
  await Deno.mkdir(fixtureDir, { recursive: true });
  await executable(
    `${fixtureDir}/mitase`,
    `#!/usr/bin/env bash
set -euo pipefail
case "\${1:-}" in
  --version) printf 'mitase 0.1.3\n' ;;
  check) printf '%s\n' "\$*" > "\${MITASE_TEST_MARKER}" ;;
  *) exit 2 ;;
esac
`,
  );
  const archiveResult = await new Deno.Command("tar", {
    args: ["-czf", archive, "mitase"],
    cwd: fixtureDir,
  }).output();
  assertEquals(
    archiveResult.success,
    true,
    new TextDecoder().decode(archiveResult.stderr),
  );
  await executable(
    `${fakeBin}/curl`,
    `#!/usr/bin/env bash
set -euo pipefail
output=""
for ((index = 1; index <= \$#; index++)); do
  if [[ "\${!index}" == "--output" ]]; then
    next=\$((index + 1))
    output="\${!next}"
  fi
done
cp "\${MITASE_TEST_ARCHIVE}" "\$output"
`,
  );
  await executable(
    `${fakeBin}/sha256sum`,
    `#!/usr/bin/env bash
printf '%s  %s\n' "\${MITASE_TEST_SHA256}" "\$1"
`,
  );
  await executable(
    `${fakeBin}/uname`,
    `#!/usr/bin/env bash
case "$1" in
  -s) printf 'Linux\n' ;;
  -m) printf 'x86_64\n' ;;
esac
`,
  );
  return {
    root: harnessRoot,
    archive,
    marker,
    env: {
      PATH: `${fakeBin}:${Deno.env.get("PATH") ?? ""}`,
      XDG_CACHE_HOME: `${harnessRoot}/cache`,
      MITASE_RELEASE_BASE_URL: "https://fixture.invalid/mitase",
      MITASE_TEST_ARCHIVE: archive,
      MITASE_TEST_SHA256:
        "9801b4ac0e65a2109854a47c47af7c52828e923af7946920a1232d7e5edb85b7",
      MITASE_TEST_MARKER: marker,
    },
  };
}

async function runScript(
  harness: Awaited<ReturnType<typeof createHarness>>,
  overrides: Record<string, string> = {},
): Promise<Deno.CommandOutput> {
  return await new Deno.Command("bash", {
    args: [scriptPath, "check", "."],
    cwd: harness.root,
    env: { ...harness.env, ...overrides },
    stdout: "piped",
    stderr: "piped",
  }).output();
}

Deno.test("Mitase is a pinned standalone consumer tool", async () => {
  const script = await readText("scripts/mitase");
  const lock = await readText("tools/mitase.lock.toml");
  for (
    const value of [
      "tools/mitase.lock.toml",
      "mitase-v${version}-${target}.tar.gz",
      "sha256_file",
      "verify_sha256",
      "verify_version",
      "MITASE_BIN",
    ]
  ) assertEquals(script.includes(value), true, value);
  for (
    const target of [
      "x86_64-unknown-linux-gnu",
      "aarch64-unknown-linux-gnu",
      "x86_64-apple-darwin",
      "aarch64-apple-darwin",
    ]
  ) assertEquals(lock.includes(`[target.${target}]`), true, target);
  for (
    const forbidden of ["cargo", "../mitase", "scripts/ci/mitase-check.sh"]
  ) {
    assertEquals(script.includes(forbidden), false, forbidden);
  }
});

Deno.test("Mitase bootstrap verifies, caches, and executes the archive", async () => {
  const harness = await createHarness();
  try {
    const first = await runScript(harness);
    assertEquals(first.success, true, new TextDecoder().decode(first.stderr));
    assertEquals(await Deno.readTextFile(harness.marker), "check .\n");

    const cachedBinary =
      `${harness.root}/cache/mitase/0.1.3/x86_64-unknown-linux-gnu/mitase`;
    await Deno.writeTextFile(cachedBinary, "corrupt cached binary\n");
    await Deno.chmod(cachedBinary, 0o755);
    const recovered = await runScript(harness);
    assertEquals(
      recovered.success,
      true,
      new TextDecoder().decode(recovered.stderr),
    );
    await Deno.remove(`${harness.root}/bin/curl`);
    const cached = await runScript(harness);
    assertEquals(cached.success, true, new TextDecoder().decode(cached.stderr));
  } finally {
    await Deno.remove(harness.root, { recursive: true });
  }
});

Deno.test("Mitase bootstrap rejects a checksum mismatch", async () => {
  const harness = await createHarness();
  try {
    await Deno.writeTextFile(harness.archive, "not the release archive\n");
    const result = await runScript(harness, {
      MITASE_TEST_SHA256: "0".repeat(64),
    });
    const stderr = new TextDecoder().decode(result.stderr);
    assertEquals(result.success, false);
    assertEquals(stderr.includes("checksum mismatch"), true, stderr);
  } finally {
    await Deno.remove(harness.root, { recursive: true });
  }
});

Deno.test("MITASE_BIN remains an explicit local development override", async () => {
  const harness = await createHarness();
  try {
    const override = `${harness.root}/override`;
    await executable(
      override,
      `#!/usr/bin/env bash
printf '%s\n' "$*" > "${harness.marker}"
`,
    );
    const result = await runScript(harness, { MITASE_BIN: override });
    assertEquals(result.success, true, new TextDecoder().decode(result.stderr));
    assertEquals(await Deno.readTextFile(harness.marker), "check .\n");
  } finally {
    await Deno.remove(harness.root, { recursive: true });
  }
});

Deno.test("Mitase bootstrap rejects unsupported hosts", async () => {
  const harness = await createHarness();
  try {
    await executable(
      `${harness.root}/bin/uname`,
      `#!/usr/bin/env bash
case "$1" in
  -s) printf 'Plan9\n' ;;
  -m) printf 'unknown\n' ;;
esac
`,
    );
    const result = await runScript(harness);
    const stderr = new TextDecoder().decode(result.stderr);
    assertEquals(result.success, false);
    assertEquals(
      stderr.includes("Unsupported Mitase release target"),
      true,
      stderr,
    );
  } finally {
    await Deno.remove(harness.root, { recursive: true });
  }
});
