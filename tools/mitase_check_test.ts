import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);

async function readText(path: string): Promise<string> {
  return await Deno.readTextFile(new URL(path, root));
}

const scriptPath = new URL(
  "../scripts/ci/mitase-check.sh",
  import.meta.url,
).pathname;

const pinnedManifestSha256 =
  "64fcca923ae28c76b078eec65cbcecaf3386c3ddc024a7f4264a18bfe5380fa7";
const pinnedSourceSha = "732c3a5e6a49394865a1ffa55178717661617880";
const pinnedCandidateId =
  "sha256:0161a266f81ef128a0d59047b86eb4168ef73b14bbd6af5773c982b13361aa7c";

function pinnedArchiveSha256(): string {
  const platform = `${Deno.build.os}:${Deno.build.arch}`;
  return {
    "darwin:aarch64":
      "6bae523ecffdfcc6976cd7b111f15d81dd5dbd86236b07a82c49ad26398654f0",
    "darwin:x86_64":
      "c80a4cb1ecff84f4b0d9db66b6e56d2dff62cce3ab28417073d95b3e3ecdd8e6",
    "linux:aarch64":
      "524ae3e2a68da6215bae6d246c686568880230092fe9147e714a8f9645faf34c",
    "linux:x86_64":
      "52090a2b5a1bd730a12629422a7c8936e3bb44e2a4179bede3ef842c4eba6ff0",
  }[platform] ?? "unsupported-platform";
}

type MitaseHarness = {
  root: string;
  fixtureRoot: string;
  fakeBin: string;
  env: Record<string, string>;
};

async function createMitaseHarness(options: {
  manifest: string;
  archive?: string;
  manifestSha256?: string;
  archiveSha256?: string;
  unsupportedHost?: boolean;
}): Promise<MitaseHarness> {
  const root = await Deno.makeTempDir({ prefix: "ugoite-mitase-import-" });
  const fixtureRoot = `${root}/fixture`;
  const fakeBin = `${root}/bin`;
  await Deno.mkdir(fixtureRoot, { recursive: true });
  await Deno.mkdir(fakeBin, { recursive: true });
  await Deno.writeTextFile(
    `${fixtureRoot}/candidate-manifest.json`,
    options.manifest,
  );
  await Deno.writeTextFile(
    `${fixtureRoot}/archive.tar.gz`,
    options.archive ?? "unused archive fixture",
  );

  await Deno.writeTextFile(
    `${fakeBin}/curl`,
    `#!/usr/bin/env bash
set -euo pipefail
output=""
for ((index = 1; index <= $#; index++)); do
  if [[ "\${!index}" == "--output" ]]; then
    next=$((index + 1))
    output="\${!next}"
  fi
done
case "\${!#}" in
  */candidate-manifest.json) cp "\${FIXTURE_ROOT}/candidate-manifest.json" "\$output" ;;
  */mitase-*.tar.gz) cp "\${FIXTURE_ROOT}/archive.tar.gz" "\$output" ;;
  *) exit 1 ;;
esac
`,
  );
  await Deno.writeTextFile(
    `${fakeBin}/sha256sum`,
    `#!/usr/bin/env bash
set -euo pipefail
case "\$1" in
  *candidate-manifest.json) printf '%s  %s\\n' "\${EXPECTED_MANIFEST_SHA256}" "\$1" ;;
  *.tar.gz) printf '%s  %s\\n' "\${EXPECTED_ARCHIVE_SHA256}" "\$1" ;;
  *) exit 1 ;;
esac
`,
  );
  if (options.unsupportedHost) {
    await Deno.writeTextFile(
      `${fakeBin}/uname`,
      `#!/usr/bin/env bash
case "\$1" in
  -s) printf 'Plan9\\n' ;;
  -m) printf 'unknown\\n' ;;
  *) printf 'Plan9\\n' ;;
esac
`,
    );
  }
  await Deno.chmod(`${fakeBin}/curl`, 0o755);
  await Deno.chmod(`${fakeBin}/sha256sum`, 0o755);
  if (options.unsupportedHost) await Deno.chmod(`${fakeBin}/uname`, 0o755);

  const path = `${fakeBin}:${Deno.env.get("PATH") ?? ""}`;
  return {
    root,
    fixtureRoot,
    fakeBin,
    env: {
      PATH: path,
      TMPDIR: root,
      MITASE_RELEASE_BASE_URL: "https://fixture.invalid/mitase",
      MITASE_ROOT: `${root}/installed`,
      FIXTURE_ROOT: fixtureRoot,
      EXPECTED_MANIFEST_SHA256: options.manifestSha256 ?? pinnedManifestSha256,
      EXPECTED_ARCHIVE_SHA256: options.archiveSha256 ?? pinnedArchiveSha256(),
    },
  };
}

async function runMitaseCheck(
  harness: MitaseHarness,
  envOverrides: Record<string, string> = {},
): Promise<Deno.CommandOutput> {
  return await new Deno.Command("bash", {
    args: [scriptPath],
    cwd: harness.root,
    env: { ...harness.env, ...envOverrides },
    stdout: "piped",
    stderr: "piped",
  }).output();
}

function validManifest(overrides: {
  candidateId?: string;
  sourceSha?: string;
} = {}): string {
  return JSON.stringify({
    candidate_id: overrides.candidateId ?? pinnedCandidateId,
    source_sha: overrides.sourceSha ?? pinnedSourceSha,
  });
}

async function withMitaseHarness(
  options: Parameters<typeof createMitaseHarness>[0],
  callback: (harness: MitaseHarness) => Promise<void>,
): Promise<void> {
  const harness = await createMitaseHarness(options);
  try {
    await callback(harness);
  } finally {
    await Deno.remove(harness.root, { recursive: true });
  }
}

Deno.test("Mitase check imports the pinned v0.1.2 release artifact", async () => {
  const script = await readText("scripts/ci/mitase-check.sh");

  for (
    const value of [
      'MITASE_RELEASE_TAG="v0.1.2"',
      'MITASE_RELEASE_TARGET="aarch64-apple-darwin"',
      'MITASE_RELEASE_TARGET="x86_64-apple-darwin"',
      'MITASE_RELEASE_TARGET="aarch64-unknown-linux-gnu"',
      'MITASE_RELEASE_TARGET="x86_64-unknown-linux-gnu"',
      'MITASE_SOURCE_SHA="732c3a5e6a49394865a1ffa55178717661617880"',
      'MITASE_CANDIDATE_ID="sha256:0161a266f81ef128a0d59047b86eb4168ef73b14bbd6af5773c982b13361aa7c"',
      'MITASE_MANIFEST_SHA256="64fcca923ae28c76b078eec65cbcecaf3386c3ddc024a7f4264a18bfe5380fa7"',
      'MITASE_ARCHIVE_SHA256="52090a2b5a1bd730a12629422a7c8936e3bb44e2a4179bede3ef842c4eba6ff0"',
      'MITASE_ARCHIVE_SHA256="6bae523ecffdfcc6976cd7b111f15d81dd5dbd86236b07a82c49ad26398654f0"',
      'MITASE_ARCHIVE_SHA256="c80a4cb1ecff84f4b0d9db66b6e56d2dff62cce3ab28417073d95b3e3ecdd8e6"',
      'MITASE_ARCHIVE_SHA256="524ae3e2a68da6215bae6d246c686568880230092fe9147e714a8f9645faf34c"',
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

Deno.test("Mitase check rejects a candidate manifest digest mismatch", async () => {
  await withMitaseHarness(
    {
      manifest: validManifest(),
      manifestSha256: "0".repeat(64),
    },
    async (harness) => {
      const result = await runMitaseCheck(harness);
      const stderr = new TextDecoder().decode(result.stderr);
      assertEquals(result.success, false);
      assertEquals(stderr.includes("release artifact checksum mismatch"), true);
      assertEquals(stderr.includes("candidate-manifest.json"), true);
    },
  );
});

Deno.test("Mitase check rejects candidate and source identity mismatches", async () => {
  for (
    const [label, manifest] of [
      [
        "candidate",
        validManifest({ candidateId: `sha256:${"0".repeat(64)}` }),
      ],
      ["source", validManifest({ sourceSha: "0".repeat(40) })],
    ] as const
  ) {
    await withMitaseHarness({ manifest }, async (harness) => {
      const result = await runMitaseCheck(harness);
      const stderr = new TextDecoder().decode(result.stderr);
      assertEquals(result.success, false, `${label}: ${stderr}`);
      assertEquals(
        stderr.includes("candidate manifest identity mismatch"),
        true,
        label,
      );
    });
  }
});

Deno.test("Mitase check rejects an invalid release archive", async () => {
  await withMitaseHarness(
    { manifest: validManifest(), archive: "not a gzip archive\n" },
    async (harness) => {
      const result = await runMitaseCheck(harness);
      const stderr = new TextDecoder().decode(result.stderr);
      assertEquals(result.success, false, stderr);
      assertEquals(stderr.includes("checksum mismatch"), false, stderr);
      assertEquals(stderr.length > 0, true, stderr);
    },
  );
});

Deno.test("Mitase check rejects unsupported hosts while keeping the local override", async () => {
  await withMitaseHarness(
    { manifest: validManifest(), unsupportedHost: true },
    async (harness) => {
      const unsupported = await runMitaseCheck(harness);
      const unsupportedStderr = new TextDecoder().decode(unsupported.stderr);
      assertEquals(unsupported.success, false);
      assertEquals(
        unsupportedStderr.includes("Unsupported Mitase release target"),
        true,
      );

      const marker = `${harness.root}/override-invocation.txt`;
      const fakeMitase = `${harness.root}/mitase-override`;
      await Deno.writeTextFile(
        fakeMitase,
        `#!/usr/bin/env bash\nprintf '%s\\n' "$*" > "${marker}"\n`,
      );
      await Deno.chmod(fakeMitase, 0o755);
      const overridden = await runMitaseCheck(harness, {
        MITASE_BIN: fakeMitase,
      });
      assertEquals(overridden.success, true);
      assertEquals((await Deno.readTextFile(marker)).trim(), "check .");
    },
  );
});
