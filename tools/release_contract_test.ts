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

Deno.test("REQ-OPS-044: version.txt is the only prepared-version authority", async () => {
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

Deno.test("REQ-OPS-044: candidate creation qualifies the acceptance corpus first", async () => {
  const releaseTool = await readText("tools/release.ts");
  for (
    const marker of [
      "qualifyAcceptanceCorpus",
      "tools/capability_report.ts",
      "test_journey_core",
      "test_journey_remote",
      "./scripts/mitase",
    ]
  ) assertEquals(releaseTool.includes(marker), true, marker);
  const qualifyStart = releaseTool.indexOf(
    "async function qualifyAcceptanceCorpus(",
  );
  const candidateStart = releaseTool.indexOf(
    "async function createCandidate(",
  );
  assertEquals(qualifyStart >= 0 && candidateStart > qualifyStart, true);
  const qualifyBody = releaseTool.slice(qualifyStart, candidateStart);
  for (const forbidden of ["playwright", "docker", "e2e"]) {
    assertEquals(qualifyBody.includes(forbidden), false, forbidden);
  }
  const candidateBody = releaseTool.slice(candidateStart, candidateStart + 800);
  assertEquals(
    candidateBody.indexOf("await qualifyAcceptanceCorpus();") >= 0,
    true,
  );
  assertEquals(
    candidateBody.indexOf("await qualifyAcceptanceCorpus();") <
      candidateBody.indexOf("UGOITE_RELEASE_CANDIDATE_PREBUILT"),
    true,
  );
});

Deno.test("REQ-OPS-044: repository-native release tasks and split workflows are present", async () => {
  const mise = await readText("mise.toml");
  for (
    const task of [
      "version:sync",
      "version:check",
      "release:prepare",
      "release:candidate",
      "release:verify-candidate",
      "release:verify-candidate-assets",
      "release:verify-candidate-smoke",
      "release:write-verification-receipt",
      "release:promote",
    ]
  ) assertEquals(mise.includes(`[tasks."${task}"]`), true, task);
  const candidate = await readText(".github/workflows/release-candidate.yml");
  const publish = await readText(".github/workflows/release-publish.yml");
  const distributionVerifier = await readText(
    "scripts/verify-release-distribution.sh",
  );
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
  assertEquals(candidate.includes("checks: read"), true);
  assertEquals(candidate.includes("ci-required"), true);
  assertEquals(candidate.includes('and .status == "completed"'), true);
  assertEquals(candidate.includes('and .conclusion == "success"'), false);
  assertEquals(
    candidate.includes('test "${source_ci_required_conclusion}" = success'),
    true,
  );
  assertEquals(candidate.includes("source_ci_required_check_run_id"), true);
  assertEquals(candidate.includes("needs.preflight.result == 'success'"), true);
  assertEquals(
    candidate.includes(
      "source_ci_required_check_run_id: ${{ steps.resolve.outputs.source_ci_required_check_run_id }}",
    ),
    true,
  );
  assertEquals(candidate.includes("release-grade:"), false);
  assertEquals(candidate.includes("mise run ci:release"), false);
  assertEquals(
    releaseTool.includes('await run("mise", ["run", "ci:release"])'),
    false,
  );
  assertEquals(releaseTool.includes("candidateIdFromManifestBytes"), true);
  assertEquals(
    releaseTool.includes(
      'artifact.kind === "cli" || artifact.kind === "release"',
    ),
    true,
  );
  assertEquals(releaseTool.includes("npm_package:"), true);
  assertEquals(releaseTool.includes("schema_version: 3"), true);
  assertEquals(
    releaseTool.includes(
      "await verifyCandidateCliArchive(candidate);",
    ),
    true,
  );
  assertEquals(
    distributionVerifier.includes("manifest.npm_package.digest"),
    true,
  );
  assertEquals(
    publish.includes("run-id: ${{ inputs.candidate_run_id }}"),
    true,
  );
  assertEquals(publish.includes("mise run release:verify-candidate"), true);
  assertEquals(publish.includes("mise run release:promote"), true);
  assertEquals(publish.includes("verify-distribution:"), true);
  assertEquals(publish.includes("publish-channel-release-notes:"), true);
  assertEquals(publish.includes("release:promote:aliases"), true);
  assertEquals(publish.includes("UGOITE_PROMOTION_DEFER_ALIASES"), false);
  assertEquals(publish.includes("inputs.candidate_id"), false);
  assertEquals(publish.includes("--candidate-id"), false);
  assertEquals(publish.includes("--candidate-run-id"), true);
  assertEquals(publish.includes("github.workflow_sha"), true);
  assertEquals(publish.includes("python3"), false);
  assertEquals(
    publish.includes("deno run -A tools/release.ts candidate-id"),
    true,
  );
  assertEquals(
    publish.includes("Verify exact candidate assets before publication"),
    true,
  );
  assertEquals(
    publish.includes("Record verification receipt for exact candidate"),
    true,
  );
  assertEquals(publish.includes("--verifier-workflow-sha"), true);
  assertEquals(publish.includes("--verification-run-id"), true);
  assertEquals(
    publish.includes("verification-receipt-${{ github.run_id }}.json"),
    true,
  );
  assertEquals(releaseTool.includes("isImmutable"), true);
  assertEquals(
    publish.includes("verify-release-container-quickstart.sh"),
    false,
  );
  assertEquals(publish.includes("verify-release-cli-quickstart.sh"), false);
  assertEquals(publish.includes("e2e:install:browsers"), false);
  assertEquals(
    publish.includes(
      "description: SHA-256 ID printed by the candidate workflow",
    ),
    false,
  );
  assertEquals(publish.includes("UGOITE_CANDIDATE_RUN_ID"), true);
  assertEquals(publish.includes("github.workflow_sha"), true);
  assertEquals(publish.includes("ref: ${{ github.workflow_sha }}"), true);
  assertEquals(publish.includes("verify-distribution:"), true);
  assertEquals(publish.includes("GH_TOKEN: ${{ github.token }}"), true);
  assertEquals(publish.includes("release:verify-candidate-assets"), true);
  assertEquals(publish.includes("verify-release-distribution.sh"), true);
  assertEquals(publish.includes("publish-channel-release-notes:"), true);
  assertEquals(publish.includes("release:promote:aliases"), true);
  assertEquals(publish.includes("UGOITE_PROMOTION_DEFER_ALIASES"), false);
  assertEquals(publish.includes("ref: main"), false);
  assertEquals(publish.includes("Install Playwright"), false);
  assertEquals(publish.includes("e2e:install"), false);
  assertEquals(publish.includes("verify-published-quickstarts"), false);
  const promoteStart = releaseTool.indexOf(
    "async function promote(\n",
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
    releaseTool.includes("candidateCliAssetPaths"),
    false,
    "promotion must not add CLI assets outside prepareReleaseAssets",
  );
  assertEquals(promoteBody.includes("...releaseAssets"), true);
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
    releaseTool.slice(stableReleaseStart, npmStart).includes("isImmutable"),
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
  assertEquals(releaseTool.includes("isImmutable"), true);
  const releaseCiStart = mise.indexOf('[tasks."ci:release"]');
  const releaseCiBody = mise.slice(releaseCiStart);
  assertEquals(releaseCiBody.includes('{ task = "ci:merge" }'), false);
  assertEquals(releaseCiBody.includes('{ task = "test:e2e" }'), false);
  assertEquals(releaseCiBody.includes('{ task = "build" }'), true);
  assertEquals(releaseCiBody.includes('{ task = "verify" }'), true);
  assertEquals(distributionVerifier.includes("isImmutable"), true);
  assertEquals(distributionVerifier.includes("npm view"), true);
  assertEquals(distributionVerifier.includes("helm pull"), true);
  assertEquals(distributionVerifier.includes("/health"), true);
  assertEquals(distributionVerifier.includes("deno task smoke"), false);
});

Deno.test("REQ-OPS-044: candidate ID is the exact manifest digest and tampering fails", async () => {
  const fixtureRoot = await Deno.makeTempDir({
    prefix: "ugoite-candidate-fixture-",
  });
  const candidateRoot = `${fixtureRoot}/candidate-fixture`;
  const preparedVersion = (await readText("version.txt")).trim();
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
      `${candidateRoot}/cli/linux/ugoite-v${preparedVersion}-x86_64-unknown-linux-gnu.tar.gz`,
      "cli",
    ),
    await writeArtifact(
      `${candidateRoot}/npm/ugoite-ugoite-${preparedVersion}.tgz`,
      "npm",
    ),
    await writeArtifact(
      `${candidateRoot}/helm/ugoite-${preparedVersion}.tgz`,
      "helm",
    ),
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
    schema_version: 4,
    contract_version: 4,
    version: preparedVersion,
    source_sha: source,
    ci_run_id: "test-run",
    source_ci_required_check_run_id: "test-ci-check",
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
        config: { version: preparedVersion },
      },
    ],
  };
  const manifestPath = `${candidateRoot}/candidate-manifest.json`;
  const manifestBytes = new TextEncoder().encode(
    `${JSON.stringify(manifest, null, 2)}\n`,
  );
  await Deno.writeFile(manifestPath, manifestBytes);
  const verify = async (
    expectedRunId = "test-run",
  ): Promise<Deno.CommandOutput> =>
    await new Deno.Command(Deno.execPath(), {
      args: [
        "run",
        "-A",
        "tools/release.ts",
        "verify-candidate",
        "--candidate",
        manifestPath,
      ],
      env: { UGOITE_CANDIDATE_RUN_ID: expectedRunId },
      stdout: "piped",
      stderr: "piped",
    }).output();
  const success = await verify();
  assertEquals(success.success, true, new TextDecoder().decode(success.stderr));
  const wrongRun = await verify("different-run");
  assertEquals(wrongRun.success, false);
  const wrongRunOutput = new TextDecoder().decode(wrongRun.stderr) +
    new TextDecoder().decode(wrongRun.stdout);
  assertEquals(
    wrongRunOutput.includes(
      "does not match requested different-run",
    ),
    true,
  );
  await Deno.writeTextFile(
    `${candidateRoot}/npm/ugoite-ugoite-${preparedVersion}.tgz`,
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

Deno.test("REQ-OPS-044: candidate writer records every promotion surface", async () => {
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
      UGOITE_SOURCE_CI_REQUIRED_CHECK_RUN_ID: "test-ci-check",
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
  assertEquals(manifest.schema_version, 4);
  assertEquals("verification" in manifest, false);
  assertEquals(
    new Set(manifest.artifacts.map((artifact) => artifact.kind)),
    new Set(["cli", "npm", "helm", "image", "release"]),
  );
});
