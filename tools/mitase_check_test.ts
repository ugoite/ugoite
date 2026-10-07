import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);
const scriptPath = new URL("../scripts/mitase", import.meta.url).pathname;

async function readText(path: string): Promise<string> {
  return await Deno.readTextFile(new URL(path, root));
}

async function yamlFiles(directory: string): Promise<string[]> {
  const files: string[] = [];
  for await (const entry of Deno.readDir(directory)) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory) {
      files.push(...await yamlFiles(path));
    } else if (entry.isFile && entry.name.endsWith(".yaml")) {
      files.push(path);
    }
  }
  return files.sort();
}

Deno.test("canonical Mitase documents use authoring v2", async () => {
  const documents = await yamlFiles(new URL("docs/mitase/", root).pathname);
  assertEquals(documents.length > 0, true);
  for (const document of documents) {
    const source = await Deno.readTextFile(document);
    const schema = source.match(/^schema:\s*(\S+)\s*$/m)?.[1];
    assertEquals(schema, "mitase/authoring/v2", document);
  }
});

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
    `${harnessRoot}/mitase-v0.2.3-x86_64-unknown-linux-gnu.tar.gz`;
  const marker = `${harnessRoot}/invocation.txt`;
  await Deno.mkdir(fakeBin, { recursive: true });
  await Deno.mkdir(fixtureDir, { recursive: true });
  await executable(
    `${fixtureDir}/mitase`,
    `#!/usr/bin/env bash
set -euo pipefail
case "\${1:-}" in
  --version) printf 'mitase 0.2.3\n' ;;
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
        "a6fd8ceea3bac8f5990381af91ceb4edeedf29fc99291b54a4a21e736a26174e",
      MITASE_TEST_MARKER: marker,
    },
  };
}

function concatChunks(chunks: Uint8Array[]): Uint8Array<ArrayBuffer> {
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.length;
  }
  return out;
}

async function runScript(
  harness: Awaited<ReturnType<typeof createHarness>>,
  overrides: Record<string, string> = {},
  options: { timeoutMs?: number } = {},
): Promise<Deno.CommandOutput> {
  // Issue #3203: a stalled fake `curl`/`uname` must fail with a diagnostic
  // instead of holding up the complete Deno tools suite. Bound every
  // bootstrap invocation with a deadline; on expiry the child is killed and
  // the caller gets a timeout error carrying the partial stderr.
  //
  // Output is pumped manually instead of `child.output()`: a killed
  // script's descendants inherit its pipes, and `output()` would wait out
  // their EOF instead of returning after the kill. Cancelling the readers
  // once the direct child exits releases our end of the pipes regardless
  // of orphaned descendants.
  const timeoutMs = options.timeoutMs ??
    Number(Deno.env.get("MITASE_BOOTSTRAP_TEST_TIMEOUT_MS") ?? "30000");
  const child = new Deno.Command("bash", {
    args: [scriptPath, "check", "."],
    cwd: harness.root,
    env: { ...harness.env, ...overrides },
    stdin: "null",
    stdout: "piped",
    stderr: "piped",
  }).spawn();
  const stdoutReader = child.stdout.getReader();
  const stderrReader = child.stderr.getReader();
  const pump = async (
    reader: ReadableStreamDefaultReader<Uint8Array>,
  ): Promise<Uint8Array<ArrayBuffer>> => {
    const chunks: Uint8Array[] = [];
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
      }
    } catch {
      // Cancelled below once the child exits; partial bytes are kept.
    } finally {
      reader.releaseLock();
    }
    return concatChunks(chunks);
  };
  const pumped = [pump(stdoutReader), pump(stderrReader)];
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    try {
      child.kill("SIGKILL");
    } catch {
      // The child already exited; its status below carries the result.
    }
  }, timeoutMs);
  try {
    const status = await child.status;
    await Promise.allSettled([stdoutReader.cancel(), stderrReader.cancel()]);
    const [stdout, stderr] = await Promise.all(pumped);
    if (timedOut) {
      throw new Error(
        `Mitase bootstrap exceeded the ${timeoutMs}ms deadline and was killed ` +
          `(code ${status.code}, signal ${status.signal ?? "none"}); stderr: ${
            new TextDecoder().decode(stderr).slice(-2000)
          }`,
      );
    }
    return {
      success: status.success,
      code: status.code,
      signal: status.signal,
      stdout,
      stderr,
    };
  } finally {
    clearTimeout(timer);
  }
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
      `${harness.root}/cache/mitase/0.2.3/x86_64-unknown-linux-gnu/mitase`;
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

Deno.test("Mitase bootstrap kills a stalled download with a diagnostic", async () => {
  const harness = await createHarness();
  try {
    await executable(
      `${harness.root}/bin/curl`,
      // A bare `sleep` keeps holding the script's stdio descriptors as a
      // descendant process: a piped harness would wait out its EOF, while
      // the bounded harness must kill and report instead.
      `#!/usr/bin/env bash
sleep 60
`,
    );
    const started = Date.now();
    let error: unknown;
    try {
      await runScript(harness, {}, { timeoutMs: 2000 });
    } catch (caught) {
      error = caught;
    }
    const elapsed = Date.now() - started;
    assertEquals(error instanceof Error, true, `${error}`);
    assertEquals(
      (error as Error).message.includes("exceeded the 2000ms deadline"),
      true,
      (error as Error).message,
    );
    // The regression is a hang: the bounded run must return far sooner
    // than the stalled fake command would on its own.
    assertEquals(elapsed < 30000, true, `elapsed ${elapsed}ms`);
  } finally {
    await Deno.remove(harness.root, { recursive: true });
  }
});
