import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);

async function readText(path: string): Promise<string> {
  return await Deno.readTextFile(new URL(path, root));
}

Deno.test("Mitase check imports the pinned v0.1.0 release artifact", async () => {
  const script = await readText("scripts/ci/mitase-check.sh");

  for (
    const value of [
      'MITASE_RELEASE_TAG="v0.1.0"',
      'MITASE_RELEASE_TARGET="aarch64-apple-darwin"',
      'MITASE_RELEASE_TARGET="x86_64-apple-darwin"',
      'MITASE_RELEASE_TARGET="aarch64-unknown-linux-gnu"',
      'MITASE_RELEASE_TARGET="x86_64-unknown-linux-gnu"',
      'MITASE_SOURCE_SHA="d0bfa043c7d4305b1a604432d2f97419db0dbb5c"',
      'MITASE_CANDIDATE_ID="sha256:771619840a3547fc167a5a98fbff3462d79b997abd2941935d6ced573eb9d82a"',
      'MITASE_MANIFEST_SHA256="dcae7fe842550efba70d99efbdfa12ba87d0263af6f98763fb22330b194b7c66"',
      'MITASE_ARCHIVE_SHA256="f56a35b350b8ac53c0c747fbaa978b7dafc30da426cb1ab618bf2984d17052ab"',
      'MITASE_ARCHIVE_SHA256="9701167c9f3dbdd545dc9c3c21f1b43db8d6c6548a0bf12c66ee0607f20b0a62"',
      'MITASE_ARCHIVE_SHA256="927b8afc9f55781693434af876e430790c0159a61f3a639a7865ccd401fa68cc"',
      'MITASE_ARCHIVE_SHA256="20e063b9243e4061a0aeb6cccfadfb8e9f52c55c85599f9d7739ad004ca153e3"',
      "candidate-manifest.json",
      "sha256_file",
      "verify_sha256",
      "grep -Fq",
      "curl --fail --location --silent --show-error --retry 3",
      "tar --extract --gzip",
      "install -m 0755",
    ]
  ) {
    assertEquals(script.includes(value), true, value);
  }

  for (
    const value of [
      "cargo install",
      "--git",
      "MITASE_REVISION",
      "MITASE_REPOSITORY",
    ]
  ) {
    assertEquals(script.includes(value), false, value);
  }

  assertEquals(
    script.indexOf('if [[ -n "${MITASE_BIN:-}" ]]') <
      script.indexOf('case "$(uname -s):$(uname -m)"'),
    true,
  );
});

Deno.test("MITASE_BIN remains a local development override", async () => {
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-mitase-bin-" });
  const marker = `${tempDir}/invocation.txt`;
  const fake = `${tempDir}/mitase`;
  try {
    await Deno.writeTextFile(
      fake,
      `#!/usr/bin/env bash\nprintf '%s\\n' "$*" > "${marker}"\n`,
    );
    await Deno.chmod(fake, 0o755);

    const result = await new Deno.Command("bash", {
      args: [
        new URL("../scripts/ci/mitase-check.sh", import.meta.url).pathname,
      ],
      cwd: tempDir,
      env: {
        PATH: Deno.env.get("PATH") ?? "",
        MITASE_BIN: fake,
      },
      stdout: "piped",
      stderr: "piped",
    }).output();

    assertEquals(result.success, true, new TextDecoder().decode(result.stderr));
    assertEquals((await Deno.readTextFile(marker)).trim(), "check .");
  } finally {
    await Deno.remove(tempDir, { recursive: true });
  }
});
