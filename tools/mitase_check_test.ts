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
  "dcae7fe842550efba70d99efbdfa12ba87d0263af6f98763fb22330b194b7c66";
const pinnedSourceSha = "d0bfa043c7d4305b1a604432d2f97419db0dbb5c";
const pinnedCandidateId =
  "sha256:771619840a3547fc167a5a98fbff3462d79b997abd2941935d6ced573eb9d82a";

function pinnedArchiveSha256(): string {
  const platform = `${Deno.build.os}:${Deno.build.arch}`;
  return {
    "darwin:aarch64":
      "9701167c9f3dbdd545dc9c3c21f1b43db8d6c6548a0bf12c66ee0607f20b0a62",
    "darwin:x86_64":
      "927b8afc9f55781693434af876e430790c0159a61f3a639a7865ccd401fa68cc",
    "linux:aarch64":
      "20e063b9243e4061a0aeb6cccfadfb8e9f52c55c85599f9d7739ad004ca153e3",
    "linux:x86_64":
      "f56a35b350b8ac53c0c747fbaa978b7dafc30da426cb1ab618bf2984d17052ab",
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
