import { assertEquals } from "@std/assert/equals";
import {
  buildCrossArtifactLedger,
  checkReleaseAssetSet,
  distributionAssetSetError,
  expectedReleaseAssetNames,
  isAllowedEvidenceAsset,
  validateCandidateRunProvenance,
  verifyCrossArtifactLedger,
  verifyDistributionManifest,
} from "./distribution.ts";
import {
  candidateIdFromManifestBytes,
  createVerificationReceipt,
  parsePublishedReleaseManifest,
  RELEASE_SMOKE_POLICY,
  sha256Hex,
} from "./release_verify.ts";

const sourceSha = "a".repeat(40);
const workflowSha = "d".repeat(40);

function releaseManifest(extraFiles: Array<{ name: string }> = []) {
  return {
    schema_version: 3,
    release_tag: "v0.1.0",
    version: "0.1.0",
    source_sha: sourceSha,
    candidate_id: `sha256:${"b".repeat(64)}`,
    files: [
      {
        name: "ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz",
        sha256: "e".repeat(64),
        size: 3,
      },
      { name: "docker-compose.release.yaml", sha256: "f".repeat(64), size: 7 },
      ...extraFiles.map((file) => ({
        ...file,
        sha256: "e".repeat(64),
        size: 1,
      })),
    ],
    image: {
      repository: "ghcr.io/ugoite/ugoite",
      digest: `sha256:${"c".repeat(64)}`,
    },
    npm_package: { name: "@ugoite/ugoite", digest: "e".repeat(64) },
    helm_chart: {
      repository: "oci://ghcr.io/ugoite/charts/ugoite",
      digest: "f".repeat(64),
    },
  };
}

async function writeDistributionFixture(
  manifestOverride: Record<string, unknown> | null,
  receiptOverride: Record<string, unknown> | null,
): Promise<{ dir: string; candidateId: string }> {
  const dir = await Deno.makeTempDir({ prefix: "ugoite-distribution-" });
  const manifest = parsePublishedReleaseManifest(
    releaseManifest() as unknown as Record<string, unknown>,
  );
  const candidateBytes = new TextEncoder().encode(
    JSON.stringify({
      schema_version: 4,
      contract_version: 4,
      version: "0.1.0",
      source_sha: sourceSha,
      ci_run_id: "candidate-123",
      source_ci_required_check_run_id: "check-456",
      artifacts: [{
        kind: "image",
        files: [],
        config: {
          repository: "ghcr.io/ugoite/ugoite",
          digest: `sha256:${"c".repeat(64)}`,
        },
      }],
    }),
  );
  const candidateId = await candidateIdFromManifestBytes(candidateBytes);
  const publicManifest = {
    ...(releaseManifest() as unknown as Record<string, unknown>),
    candidate_id: candidateId,
    ...(manifestOverride ?? {}),
  };
  await Deno.writeTextFile(
    `${dir}/release-manifest.json`,
    JSON.stringify(publicManifest),
  );
  await Deno.writeTextFile(
    `${dir}/candidate-manifest.json`,
    new TextDecoder().decode(candidateBytes),
  );
  await Deno.writeTextFile(`${dir}/candidate-id.txt`, `${candidateId}\n`);
  const receipt = {
    schema_version: 1,
    candidate_id: candidateId,
    candidate_run_id: "candidate-123",
    verifier_workflow_sha: workflowSha,
    verification_run_id: "verification-789",
    policy: RELEASE_SMOKE_POLICY,
    result: "passed",
    ...(receiptOverride ?? {}),
  };
  await Deno.writeTextFile(
    `${dir}/verification-receipt.json`,
    JSON.stringify(receipt),
  );
  // Spot-check files with exact bytes matching the manifest digests.
  void manifest;
  return { dir, candidateId };
}

async function assertFails(
  action: () => unknown | Promise<unknown>,
  message: string,
) {
  try {
    await action();
    throw new Error(`expected failure containing: ${message}`);
  } catch (error) {
    if (!(error instanceof Error) || !error.message.includes(message)) {
      throw error;
    }
  }
}

Deno.test("distribution verifier compares exact digests without live registries", async () => {
  const bytes = new TextEncoder().encode("abc");
  const digest = await sha256Hex(bytes);
  const manifest = parsePublishedReleaseManifest({
    ...releaseManifest(),
    files: [{ name: "asset.bin", sha256: digest, size: bytes.byteLength }],
  });
  const { expected } = checkReleaseAssetSet(
    [
      ...manifest.files.map((file) => file.name),
      "candidate-manifest.json",
      "candidate-id.txt",
      "release-manifest.json",
    ],
    manifest,
  );
  assertEquals(expected.includes("asset.bin"), true);
  // Mismatched digest fixture.
  const { dir, candidateId } = await writeDistributionFixture(null, null);
  await assertFails(
    () =>
      verifyDistributionManifest({
        manifestPath: `${dir}/release-manifest.json`,
        candidateManifestPath: `${dir}/candidate-manifest.json`,
        candidateIdPath: `${dir}/candidate-id.txt`,
        receiptPath: `${dir}/verification-receipt.json`,
        releaseTag: "v0.1.0",
        version: "0.1.0",
        sourceSha,
        imageRepository: "ghcr.io/ugoite/ugoite",
        candidateId: `sha256:${"0".repeat(64)}`,
      }),
    "does not match promotion input",
  );
  void candidateId;
});

Deno.test("distribution verifier rejects missing digests", async () => {
  const manifest = parsePublishedReleaseManifest(
    releaseManifest() as unknown as Record<string, unknown>,
  );
  const check = checkReleaseAssetSet(["docker-compose.release.yaml"], manifest);
  assertEquals(
    check.missing.includes("ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"),
    true,
  );
  assertEquals(check.unexpected.length, 0);
  await assertFails(async () => {
    if (check.missing.length > 0 || check.unexpected.length > 0) {
      throw new Error(distributionAssetSetError(check));
    }
  }, "missing release assets");
});

Deno.test("distribution verifier rejects unexpected assets but allows receipts", async () => {
  const manifest = parsePublishedReleaseManifest(
    releaseManifest() as unknown as Record<string, unknown>,
  );
  const base = expectedReleaseAssetNames(manifest);
  assertEquals(isAllowedEvidenceAsset("verification-receipt-123.json"), true);
  assertEquals(isAllowedEvidenceAsset("candidate-manifest.json"), true);
  const allowed = checkReleaseAssetSet([
    ...base,
    "verification-receipt-999.json",
  ], manifest);
  assertEquals(allowed.missing.length, 0);
  assertEquals(allowed.unexpected.length, 0);
  const unexpected = checkReleaseAssetSet([...base, "evil-binary"], manifest);
  assertEquals(unexpected.unexpected, ["evil-binary"]);
  await assertFails(async () => {
    throw new Error(distributionAssetSetError(unexpected));
  }, "unexpected release assets");
});

Deno.test("candidate provenance requires Release Candidate workflow, SHA, and success", async () => {
  validateCandidateRunProvenance(
    {
      workflowName: "Release Candidate",
      headSha: sourceSha,
      status: "completed",
      conclusion: "success",
    },
    { sourceSha },
  );
  await assertFails(async () => {
    validateCandidateRunProvenance(
      {
        workflowName: "CI",
        headSha: sourceSha,
        status: "completed",
        conclusion: "success",
      },
      { sourceSha },
    );
  }, "is not Release Candidate");
  await assertFails(async () => {
    validateCandidateRunProvenance(
      {
        workflowName: "Release Candidate",
        headSha: "b".repeat(40),
        status: "completed",
        conclusion: "success",
      },
      { sourceSha },
    );
  }, "does not match expected");
  await assertFails(async () => {
    validateCandidateRunProvenance(
      {
        workflowName: "Release Candidate",
        headSha: sourceSha,
        status: "completed",
        conclusion: "failure",
      },
      { sourceSha },
    );
  }, "is not success");
});

Deno.test("verification receipt binding matches verifier SHA and run ID", async () => {
  const candidateId = `sha256:${"b".repeat(64)}`;
  const receipt = createVerificationReceipt({
    candidateId,
    candidateRunId: "candidate-123",
    verifierWorkflowSha: workflowSha,
    verificationRunId: "verification-789",
    policy: RELEASE_SMOKE_POLICY,
  });
  const { validateVerificationReceipt } = await import("./release_verify.ts");
  validateVerificationReceipt(receipt, {
    candidateId,
    candidateRunId: "candidate-123",
    verifierWorkflowSha: workflowSha,
    verificationRunId: "verification-789",
    policy: RELEASE_SMOKE_POLICY,
  });
  await assertFails(async () => {
    validateVerificationReceipt(receipt, {
      candidateId,
      candidateRunId: "candidate-123",
      verifierWorkflowSha: "e".repeat(40),
      verificationRunId: "verification-789",
    });
  }, "verifier workflow SHA does not match");
  await assertFails(async () => {
    validateVerificationReceipt(receipt, {
      candidateId,
      candidateRunId: "candidate-123",
      verifierWorkflowSha: workflowSha,
      verificationRunId: "other-run",
    });
  }, "verification run ID does not match");
});

Deno.test("cross-artifact ledger binds tag, source, digests, and subjects", async () => {
  const ledger = buildCrossArtifactLedger({
    tag: "v0.1.0",
    version: "0.1.0",
    sourceSha,
    candidateId: `sha256:${"b".repeat(64)}`,
    cliArchive: {
      name: "ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz",
      sha256: "e".repeat(64),
      size: 3,
    },
    npmTarball: { spec: "@ugoite/ugoite@0.1.0", sha256: "f".repeat(64) },
    helmArchive: { name: "ugoite-0.1.0.tgz", sha256: "a".repeat(64), size: 5 },
    image: {
      repository: "ghcr.io/ugoite/ugoite",
      digest: `sha256:${"c".repeat(64)}`,
    },
  });
  assertEquals(ledger.tag, "v0.1.0");
  assertEquals(ledger.entries.length, 4);
  assertEquals(ledger.helm_oci_note.includes("OCI"), true);
  verifyCrossArtifactLedger(ledger, {
    cli: {
      digest: "e".repeat(64),
      subject: "ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz",
    },
    npm: { digest: "f".repeat(64), subject: "@ugoite/ugoite@0.1.0" },
    helm: { digest: "a".repeat(64), subject: "ugoite-0.1.0.tgz" },
    image: {
      digest: `sha256:${"c".repeat(64)}`,
      subject: "ghcr.io/ugoite/ugoite:0.1.0",
    },
  });
  await assertFails(async () => {
    verifyCrossArtifactLedger(ledger, {
      cli: {
        digest: "0".repeat(64),
        subject: "ugoite-v0.1.0-x86_64-unknown-linux-gnu.tar.gz",
      },
      npm: { digest: "f".repeat(64), subject: "@ugoite/ugoite@0.1.0" },
      helm: { digest: "a".repeat(64), subject: "ugoite-0.1.0.tgz" },
      image: {
        digest: `sha256:${"c".repeat(64)}`,
        subject: "ghcr.io/ugoite/ugoite:0.1.0",
      },
    });
  }, "differs from");
});
