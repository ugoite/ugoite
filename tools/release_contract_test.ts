import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);

async function readText(path: string): Promise<string> {
  return await Deno.readTextFile(new URL(path, root));
}

async function digest(bytes: Uint8Array): Promise<string> {
  const hash = await crypto.subtle.digest("SHA-256", bytes as BufferSource);
  return [...new Uint8Array(hash)].map((byte) =>
    byte.toString(16).padStart(2, "0")
  ).join("");
}

async function writeArtifact(
  path: string,
  contents: string,
): Promise<{ path: string; sha256: string; size: number }> {
  const bytes = new TextEncoder().encode(contents);
  await Deno.writeFile(path, bytes);
  return {
    path: path.slice(
      path.lastIndexOf("candidate-fixture/") + "candidate-fixture/".length,
    ),
    sha256: await digest(bytes),
    size: bytes.byteLength,
  };
}

Deno.test("REQ-OPS-043: version.txt is the only prepared-version authority", async () => {
  const version = (await readText("version.txt")).trim();
  const cargo = await readText("Cargo.toml");
  const packageJson = JSON.parse(
    await readText("packages/ugoite/package.json"),
  ) as { version?: string };
  const chart = await readText("charts/ugoite/Chart.yaml");
  const values = await readText("charts/ugoite/values.yaml");
  assertEquals(
    version,
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(version)?.[0],
  );
  assertEquals(
    cargo.match(/\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/)?.[1],
    version,
  );
  assertEquals(packageJson.version, version);
  assertEquals(chart.match(/^version:\s*([^\n]+)$/m)?.[1], version);
  assertEquals(chart.match(/^appVersion:\s*"?([^"\n]+)"?$/m)?.[1], version);
  assertEquals(values.match(/^\x20\x20tag:\s*([^\n]+)$/m)?.[1], version);
  try {
    await Deno.stat(".release-please-manifest.json");
    throw new Error("legacy release manifest must not exist");
  } catch (error) {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
  }
});

Deno.test("REQ-OPS-043: repository-native release tasks and split workflows are present", async () => {
  const mise = await readText("mise.toml");
  for (
    const task of [
      "version:sync",
      "version:check",
      "release:prepare",
      "release:candidate",
      "release:verify-candidate",
      "release:promote",
    ]
  ) assertEquals(mise.includes(`[tasks."${task}"]`), true, task);
  const candidate = await readText(".github/workflows/release-candidate.yml");
  const publish = await readText(".github/workflows/release-publish.yml");
  const releaseTool = await readText("tools/release.ts");
  for (const text of [candidate, publish]) {
    assertEquals(text.includes("permissions: {}"), true);
    assertEquals(text.includes("source_sha"), true);
  }
  assertEquals(candidate.includes("mise run release:candidate"), true);
  assertEquals(candidate.includes("candidate-manifest.json"), true);
  assertEquals(candidate.includes("docker-compose.release.yaml"), true);
  assertEquals(
    candidate.includes(
      "candidate-${{ needs.preflight.outputs.source_short }}-${{ github.run_id }}",
    ),
    true,
  );
  assertEquals(
    publish.includes("run-id: ${{ inputs.candidate_run_id }}"),
    true,
  );
  assertEquals(publish.includes("mise run release:verify-candidate"), true);
  assertEquals(publish.includes("mise run release:promote"), true);
  assertEquals(publish.includes("verify-published-quickstarts:"), true);
  assertEquals(publish.includes("publish-channel-release-notes:"), true);
  assertEquals(publish.includes("release:promote:aliases"), true);
  assertEquals(publish.includes("UGOITE_PROMOTION_DEFER_ALIASES"), false);
  const promoteStart = releaseTool.indexOf(
    "async function promote(candidate: VerifiedCandidate)",
  );
  const aliasesStart = releaseTool.indexOf(
    "async function promoteAliases(candidate: VerifiedCandidate)",
  );
  assertEquals(promoteStart >= 0 && aliasesStart > promoteStart, true);
  assertEquals(
    releaseTool.slice(promoteStart, aliasesStart).includes("promoteAliases"),
    false,
  );
  const promoteBody = releaseTool.slice(promoteStart, aliasesStart);
  assertEquals(
    promoteBody.indexOf("publishContainer(candidate)") <
      promoteBody.indexOf("ensureStableRelease("),
    true,
  );
  const stableReleaseStart = releaseTool.indexOf(
    "async function ensureStableRelease(",
  );
  const npmStart = releaseTool.indexOf("async function publishNpm(");
  assertEquals(
    releaseTool.slice(stableReleaseStart, npmStart).includes('"--draft"'),
    true,
  );
  assertEquals(
    releaseTool.slice(npmStart).includes('"--tag", "latest"'),
    false,
  );
  const aliasesBody = releaseTool.slice(
    aliasesStart,
    releaseTool.indexOf("async function ensureDraftRelease"),
  );
  assertEquals(aliasesBody.includes("sourceTag"), false);
  assertEquals(releaseTool.includes("candidateDraftTag(candidate)"), true);
  assertEquals(
    /cargo build|npm pack|helm package|docker\/build-push-action|mise run build:/
      .test(publish),
    false,
  );
});

Deno.test("REQ-OPS-043: candidate ID is the exact manifest digest and tampering fails", async () => {
  const fixtureRoot = await Deno.makeTempDir({
    prefix: "ugoite-candidate-fixture-",
  });
  const candidateRoot = `${fixtureRoot}/candidate-fixture`;
  await Deno.mkdir(`${candidateRoot}/cli/linux`, { recursive: true });
  await Deno.mkdir(`${candidateRoot}/npm`, { recursive: true });
  await Deno.mkdir(`${candidateRoot}/helm`, { recursive: true });
  const sourceSha = await new Deno.Command("git", {
    args: ["rev-parse", "HEAD"],
    stdout: "piped",
  }).output();
  const source = new TextDecoder().decode(sourceSha.stdout).trim();
  const files = [
    await writeArtifact(
      `${candidateRoot}/cli/linux/ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`,
      "cli",
    ),
    await writeArtifact(`${candidateRoot}/npm/ugoite-ugoite-0.1.0.tgz`, "npm"),
    await writeArtifact(`${candidateRoot}/helm/ugoite-0.1.0.tgz`, "helm"),
    await writeArtifact(
      `${candidateRoot}/docker-compose.release.yaml`,
      "compose",
    ),
    await writeArtifact(
      `${candidateRoot}/docker-compose.release.yaml.sha256`,
      `${await digest(
        new TextEncoder().encode("compose"),
      )}  docker-compose.release.yaml\n`,
    ),
  ];
  const manifest = {
    schema_version: 2,
    contract_version: 2,
    version: "0.1.0",
    source_sha: source,
    ci_run_id: "test-run",
    verification: { release_grade: "passed" },
    artifacts: [
      {
        kind: "cli",
        files: [files[0]],
        config: { platform: "x86_64-unknown-linux-gnu" },
      },
      { kind: "npm", files: [files[1]], config: { package: "@ugoite/ugoite" } },
      { kind: "helm", files: [files[2]], config: { chart: "ugoite" } },
      {
        kind: "image",
        files: [],
        config: {
          repository: "ghcr.io/ugoite/ugoite",
          tag: "sha-test",
          digest: `sha256:${"a".repeat(64)}`,
        },
      },
      {
        kind: "release",
        files: [files[3], files[4]],
        config: { version: "0.1.0" },
      },
    ],
  };
  const manifestPath = `${candidateRoot}/candidate-manifest.json`;
  const manifestBytes = new TextEncoder().encode(
    `${JSON.stringify(manifest, null, 2)}\n`,
  );
  await Deno.writeFile(manifestPath, manifestBytes);
  const candidateId = `sha256:${await digest(manifestBytes)}`;

  const verify = async (): Promise<Deno.CommandOutput> =>
    await new Deno.Command(Deno.execPath(), {
      args: [
        "run",
        "-A",
        "tools/release.ts",
        "verify-candidate",
        "--candidate",
        manifestPath,
        "--candidate-id",
        candidateId,
      ],
      stdout: "piped",
      stderr: "piped",
    }).output();
  const success = await verify();
  assertEquals(success.success, true, new TextDecoder().decode(success.stderr));
  await Deno.writeTextFile(
    `${candidateRoot}/npm/ugoite-ugoite-0.1.0.tgz`,
    "tampered",
  );
  const failure = await verify();
  assertEquals(failure.success, false);
  assertEquals(
    new TextDecoder().decode(failure.stderr).includes(
      "candidate digest mismatch",
    ),
    true,
  );
});

Deno.test("REQ-OPS-043: candidate writer records every promotion surface", async () => {
  const fixtureRoot = await Deno.makeTempDir({
    prefix: "ugoite-candidate-writer-",
  });
  const artifactRoot = `${fixtureRoot}/artifacts`;
  await Deno.mkdir(`${artifactRoot}/cli/linux`, { recursive: true });
  await Deno.mkdir(`${artifactRoot}/npm`, { recursive: true });
  await Deno.mkdir(`${artifactRoot}/helm`, { recursive: true });
  await Deno.writeTextFile(
    `${artifactRoot}/cli/linux/ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`,
    "cli",
  );
  await Deno.writeTextFile(
    `${artifactRoot}/npm/ugoite-ugoite-0.1.0.tgz`,
    "npm",
  );
  await Deno.writeTextFile(`${artifactRoot}/helm/ugoite-0.1.0.tgz`, "helm");
  await Deno.writeTextFile(
    `${artifactRoot}/docker-compose.release.yaml`,
    "compose",
  );
  await Deno.writeTextFile(
    `${artifactRoot}/docker-compose.release.yaml.sha256`,
    "placeholder",
  );
  const sourceSha = await new Deno.Command("git", {
    args: ["rev-parse", "HEAD"],
    stdout: "piped",
  }).output();
  const source = new TextDecoder().decode(sourceSha.stdout).trim();
  const output = await new Deno.Command(Deno.execPath(), {
    args: ["run", "-A", "tools/artifacts.ts", "write-candidate-manifest"],
    env: {
      UGOITE_ARTIFACT_ROOT: artifactRoot,
      UGOITE_SOURCE_SHA: source,
      UGOITE_CI_RUN_ID: "test-run",
      UGOITE_RELEASE_GRADE: "passed",
      UGOITE_CONTAINER_TAG: "sha-test",
      UGOITE_CONTAINER_DIGEST: `sha256:${"a".repeat(64)}`,
    },
    stdout: "piped",
    stderr: "piped",
  }).output();
  assertEquals(output.success, true, new TextDecoder().decode(output.stderr));
  const manifest = JSON.parse(
    await Deno.readTextFile(`${artifactRoot}/candidate-manifest.json`),
  ) as { artifacts: Array<{ kind: string }>; schema_version: number };
  assertEquals(manifest.schema_version, 2);
  assertEquals(
    new Set(manifest.artifacts.map((artifact) => artifact.kind)),
    new Set(["cli", "npm", "helm", "image", "release"]),
  );
});
