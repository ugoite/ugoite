/**
 * Distribution verifier core for published GitHub Release verification.
 *
 * This module owns the stable, network-free verification logic used by
 * `scripts/verify-release-distribution.sh`. The shell script keeps only
 * Docker/curl/registry orchestration (downloads, `gh release view`, `npm view`,
 * `helm pull`, `docker buildx imagetools inspect`, container health, CLI
 * installer); every manifest parse, expected-artifact-set computation, exact
 * digest comparison, receipt validation, and mismatch diagnostic lives here so
 * it can be unit-tested without live registries.
 *
 * No product behavior change: this is pipeline hardening only.
 */
import {
  candidateIdFromManifestBytes,
  findPublishedReleaseFile,
  parseCandidateManifest,
  parsePublishedReleaseManifest,
  parseVerificationReceipt,
  type PublishedReleaseManifest,
  RELEASE_SMOKE_POLICY,
  sha256Hex,
  validatePublishedReleaseManifest,
  validateVerificationReceipt,
} from "./release_verify.ts";

/** Evidence assets that are never listed in `release-manifest.json` files. */
export const DISTRIBUTION_EVIDENCE_ASSETS = [
  "candidate-manifest.json",
  "candidate-id.txt",
  "release-manifest.json",
] as const;

export const VERIFICATION_RECEIPT_PATTERN =
  /^verification-receipt-[^/]+\.json$/;

/** Workflow identity expected for candidate provenance preflight. */
export const CANDIDATE_WORKFLOW_NAME = "Release Candidate";

export type CandidateRunProvenance = {
  /** Workflow name reported by the GitHub Actions API (e.g. `run.name`). */
  workflowName?: string;
  /** Workflow file path reported by the API (e.g. `run.path`). */
  workflowPath?: string;
  /** Head commit SHA reported by the API (e.g. `run.head_sha`). */
  headSha?: string;
  /** Run status reported by the API (e.g. `run.status`). */
  status?: string;
  /** Run conclusion reported by the API (e.g. `run.conclusion`). */
  conclusion?: string;
};

export type ExpectedCandidateProvenance = {
  workflowName?: string;
  workflowPath?: string;
  sourceSha: string;
};

export type ReleaseAssetSetCheck = {
  expected: string[];
  missing: string[];
  unexpected: string[];
};

export type DistributionManifestExpectation = {
  releaseTag: string;
  version: string;
  sourceSha: string;
  imageRepository: string;
  candidateId: string;
};

export type DistributionReceiptExpectation = {
  candidateId: string;
  candidateRunId: string;
  verifierWorkflowSha?: string;
  verificationRunId?: string;
  policy?: string;
};

export type CrossArtifactLedgerEntry = {
  surface: "cli" | "npm" | "helm" | "image";
  /** Machine-readable digest: lowercase hex for archives, `sha256:` for images. */
  digest: string;
  /** Published subject: Release asset name, npm spec, Helm ref, or image ref. */
  subject: string;
  size?: number;
};

export type CrossArtifactLedger = {
  schema_version: 1;
  tag: string;
  version: string;
  source_sha: string;
  candidate_id: string;
  entries: CrossArtifactLedgerEntry[];
  /**
   * Helm `.tgz` archives are content-addressed by file SHA-256 while the OCI
   * registry addresses the same chart version by descriptor digest. The ledger
   * records the archive file digest (what `helm pull` writes to disk); the OCI
   * descriptor digest is a registry transport identity for the same bytes and
   * must be verified by comparing pulled bytes, never by equating the two
   * digest strings.
   */
  helm_oci_note: string;
};

if (import.meta.main) await main(Deno.args);

async function main(args: string[]): Promise<void> {
  const [command, ...rest] = args;
  switch (command) {
    case "list-files":
      console.log(
        (await readReleaseManifest(flagValue(rest, "--manifest") ?? "")).files
          .map((
            file,
          ) => file.name).join("\n"),
      );
      break;
    case "manifest-digest":
      console.log(
        await manifestFieldDigest(
          await readReleaseManifest(flagValue(rest, "--manifest") ?? ""),
          flagValue(rest, "--field") ?? "",
        ),
      );
      break;
    case "verify-file":
      await verifySingleFile(
        await readReleaseManifest(flagValue(rest, "--manifest") ?? ""),
        flagValue(rest, "--name") ?? "",
        flagValue(rest, "--path") ?? "",
      );
      break;
    case "verify-manifest": {
      await verifyDistributionManifest({
        manifestPath: flagValue(rest, "--manifest") ?? "",
        candidateManifestPath: flagValue(rest, "--candidate-manifest") ?? "",
        candidateIdPath: flagValue(rest, "--candidate-id-path") ?? "",
        receiptPath: flagValue(rest, "--receipt") ?? "",
        composePath: flagValue(rest, "--compose-path"),
        composeChecksumPath: flagValue(rest, "--compose-checksum-path"),
        cliArchivePath: flagValue(rest, "--cli-archive-path"),
        cliArchiveName: flagValue(rest, "--cli-archive-name"),
        releaseTag: flagValue(rest, "--release-tag") ?? "",
        version: flagValue(rest, "--version") ?? "",
        sourceSha: flagValue(rest, "--source-sha") ?? "",
        imageRepository: flagValue(rest, "--image-repository") ?? "",
        candidateId: flagValue(rest, "--candidate-id") ?? "",
        verifierWorkflowSha: flagValue(rest, "--verifier-workflow-sha"),
        verificationRunId: flagValue(rest, "--verification-run-id"),
      });
      break;
    }
    case "check-asset-set": {
      const manifest = await readReleaseManifest(
        flagValue(rest, "--manifest") ?? "",
      );
      const actual = await readAssetNameList(flagValue(rest, "--assets") ?? "");
      const check = checkReleaseAssetSet(actual, manifest);
      if (check.missing.length > 0 || check.unexpected.length > 0) {
        throw new Error(distributionAssetSetError(check));
      }
      console.log(
        `release asset set OK: ${check.expected.length} expected, ${actual.length} actual`,
      );
      break;
    }
    case "build-ledger": {
      // Rerunnable from the exact tag: every subject/digest is derived from
      // immutable published identities, so the same tag always yields the same
      // ledger bytes. Publish attestations (CLI/npm build provenance) are
      // verified separately with `gh attestation verify` where bundles exist;
      // absence of an attestation bundle never fails ledger construction.
      const ledger = buildCrossArtifactLedger({
        tag: requiredFlag(rest, "--tag"),
        version: requiredFlag(rest, "--version"),
        sourceSha: requiredFlag(rest, "--source-sha"),
        candidateId: requiredFlag(rest, "--candidate-id"),
        cliArchive: {
          name: requiredFlag(rest, "--cli-name"),
          sha256: requiredFlag(rest, "--cli-sha256"),
          size: Number(requiredFlag(rest, "--cli-size")),
        },
        npmTarball: {
          spec: requiredFlag(rest, "--npm-spec"),
          sha256: requiredFlag(rest, "--npm-sha256"),
        },
        helmArchive: {
          name: requiredFlag(rest, "--helm-name"),
          sha256: requiredFlag(rest, "--helm-sha256"),
          size: Number(requiredFlag(rest, "--helm-size")),
        },
        image: {
          repository: requiredFlag(rest, "--image-repository"),
          digest: requiredFlag(rest, "--image-digest"),
        },
      });
      const output = flagValue(rest, "--output");
      const text = `${JSON.stringify(ledger, null, 2)}\n`;
      if (output) await Deno.writeTextFile(output, text);
      else console.log(text.trimEnd());
      break;
    }
    case "verify-ledger": {
      const ledger = JSON.parse(
        await Deno.readTextFile(requiredFlag(rest, "--ledger")),
      ) as CrossArtifactLedger;
      const observedPath = flagValue(rest, "--observed");
      const observed: Record<string, { digest: string; subject: string }> =
        observedPath
          ? JSON.parse(await Deno.readTextFile(observedPath))
          : Object.fromEntries(
            ledger.entries.map((entry) => [
              entry.surface,
              { digest: entry.digest, subject: entry.subject },
            ]),
          );
      verifyCrossArtifactLedger(ledger, observed);
      console.log(`ledger OK for ${ledger.tag}`);
      break;
    }
    default:
      throw new Error(
        "usage: deno run -A tools/distribution.ts <list-files|manifest-digest|verify-file|verify-manifest|check-asset-set|build-ledger|verify-ledger> [flags]",
      );
  }
}

/** True for whitelisted evidence artifacts that are never manifest files. */
export function isAllowedEvidenceAsset(name: string): boolean {
  if ((DISTRIBUTION_EVIDENCE_ASSETS as readonly string[]).includes(name)) {
    return true;
  }
  return VERIFICATION_RECEIPT_PATTERN.test(name);
}

/**
 * Complete expected GitHub Release asset name set: every manifest file plus
 * whitelisted evidence artifacts (candidate manifest/ID, release manifest, and
 * run-scoped verification receipts).
 */
export function expectedReleaseAssetNames(
  manifest: PublishedReleaseManifest,
): string[] {
  return [
    ...manifest.files.map((file) => file.name),
    ...DISTRIBUTION_EVIDENCE_ASSETS,
  ];
}

/**
 * Compare a complete published Release asset name set against the expected
 * set. Unexpected names (outside manifest files and whitelisted evidence) and
 * missing expected names are both verification failures.
 */
export function checkReleaseAssetSet(
  actualNames: string[],
  manifest: PublishedReleaseManifest,
): ReleaseAssetSetCheck {
  const expected = expectedReleaseAssetNames(manifest);
  const expectedSet = new Set(expected);
  const actualSet = new Set(actualNames);
  const missing = expected.filter((name) => !actualSet.has(name));
  const unexpected = actualNames.filter((name) =>
    !expectedSet.has(name) && !isAllowedEvidenceAsset(name)
  );
  return { expected, missing, unexpected };
}

export function distributionAssetSetError(check: ReleaseAssetSetCheck): string {
  const parts: string[] = [];
  if (check.missing.length > 0) {
    parts.push(`missing release assets: ${check.missing.join(", ")}`);
  }
  if (check.unexpected.length > 0) {
    parts.push(`unexpected release assets: ${check.unexpected.join(", ")}`);
  }
  return `distribution validation failed: ${parts.join("; ")}`;
}

/** Exact digest+size comparison of one downloaded asset against the manifest. */
export async function verifySingleFile(
  manifest: PublishedReleaseManifest,
  name: string,
  path: string,
): Promise<void> {
  if (!name) {
    throw new Error("distribution validation failed: asset name is required");
  }
  if (!path) {
    throw new Error(
      `distribution validation failed: asset path is required for ${name}`,
    );
  }
  const record = findPublishedReleaseFile(manifest, name);
  let bytes: Uint8Array;
  try {
    bytes = await Deno.readFile(path);
  } catch {
    throw new Error(
      `distribution validation failed: ${name} was not downloaded`,
    );
  }
  await assertBytesMatchManifest(name, bytes, record.sha256, record.size);
}

export async function assertBytesMatchManifest(
  name: string,
  bytes: Uint8Array,
  expectedSha256: string,
  expectedSize: number,
): Promise<void> {
  if (bytes.byteLength !== expectedSize) {
    throw new Error(
      `distribution validation failed: ${name} size ${bytes.byteLength} differs from manifest ${expectedSize}`,
    );
  }
  const actual = await sha256Hex(bytes);
  if (actual !== expectedSha256) {
    throw new Error(
      `distribution validation failed: ${name} digest ${actual} differs from manifest ${expectedSha256}`,
    );
  }
}

/**
 * Stable distribution-manifest verification formerly embedded as an inline
 * `deno eval` in `scripts/verify-release-distribution.sh`: manifest parse,
 * expected-identity comparison, candidate-ID derivation, receipt validation
 * (including verifier-workflow-SHA and verification-run-ID binding when
 * provided), and exact digest comparison for the compose/CLI spot-check files.
 */
export async function verifyDistributionManifest(input: {
  manifestPath: string;
  candidateManifestPath: string;
  candidateIdPath: string;
  receiptPath: string;
  composePath?: string;
  composeChecksumPath?: string;
  cliArchivePath?: string;
  cliArchiveName?: string;
  releaseTag: string;
  version: string;
  sourceSha: string;
  imageRepository: string;
  candidateId: string;
  verifierWorkflowSha?: string;
  verificationRunId?: string;
}): Promise<void> {
  const fail = (message: string): never => {
    throw new Error(`distribution validation failed: ${message}`);
  };
  let manifestText: string;
  try {
    manifestText = await Deno.readTextFile(input.manifestPath);
  } catch {
    fail(`release manifest was not found at ${input.manifestPath}`);
  }
  const manifest = parsePublishedReleaseManifest(
    JSON.parse(manifestText!),
  );
  let candidateBytes: Uint8Array;
  try {
    candidateBytes = await Deno.readFile(input.candidateManifestPath);
  } catch {
    fail(`candidate manifest was not found at ${input.candidateManifestPath}`);
  }
  const candidate = parseCandidateManifest(candidateBytes!);
  let receiptText: string;
  try {
    receiptText = await Deno.readTextFile(input.receiptPath);
  } catch {
    fail(`verification receipt was not found at ${input.receiptPath}`);
  }
  const receipt = parseVerificationReceipt(
    JSON.parse(receiptText!),
  );
  const candidateId = await candidateIdFromManifestBytes(candidateBytes!);
  const expectation: DistributionManifestExpectation = {
    releaseTag: input.releaseTag,
    version: input.version,
    sourceSha: input.sourceSha,
    imageRepository: input.imageRepository,
    candidateId,
  };
  try {
    validatePublishedReleaseManifest(manifest, {
      releaseTag: expectation.releaseTag,
      version: expectation.version,
      sourceSha: expectation.sourceSha,
      imageRepository: expectation.imageRepository,
      candidateId: expectation.candidateId,
    });
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }
  if (candidateId !== input.candidateId) {
    fail("candidate manifest digest does not match promotion input");
  }
  let publishedCandidateId: string;
  try {
    publishedCandidateId = (await Deno.readTextFile(input.candidateIdPath))
      .trim();
  } catch {
    fail(`candidate ID asset was not found at ${input.candidateIdPath}`);
  }
  if (candidateId !== publishedCandidateId!) {
    fail("published candidate ID asset differs from candidate manifest");
  }
  const receiptExpectation: DistributionReceiptExpectation = {
    candidateId,
    candidateRunId: candidate.ci_run_id,
    policy: RELEASE_SMOKE_POLICY,
  };
  if (input.verifierWorkflowSha) {
    receiptExpectation.verifierWorkflowSha = input.verifierWorkflowSha;
  }
  if (input.verificationRunId) {
    receiptExpectation.verificationRunId = input.verificationRunId;
  }
  try {
    validateVerificationReceipt(receipt, receiptExpectation);
  } catch (error) {
    fail(error instanceof Error ? error.message : String(error));
  }
  const spotChecks: Array<[string, string]> = [];
  if (input.composePath) {
    spotChecks.push(["docker-compose.release.yaml", input.composePath]);
  }
  if (input.composeChecksumPath) {
    spotChecks.push([
      "docker-compose.release.yaml.sha256",
      input.composeChecksumPath,
    ]);
  }
  if (input.cliArchivePath && input.cliArchiveName) {
    spotChecks.push([input.cliArchiveName, input.cliArchivePath]);
  }
  for (const [name, path] of spotChecks) {
    try {
      await verifySingleFile(manifest, name, path);
    } catch (error) {
      fail(
        error instanceof Error
          ? error.message.replace(/^distribution validation failed: /, "")
          : String(error),
      );
    }
  }
}

/**
 * Publish preflight provenance check via the GitHub Actions API: the candidate
 * run must be the Release Candidate workflow, target the expected source SHA,
 * and have a successful conclusion. The candidate manifest stays the artifact
 * authority; this check only refuses promotion when run metadata disagrees.
 */
export function validateCandidateRunProvenance(
  run: CandidateRunProvenance,
  expected: ExpectedCandidateProvenance,
): void {
  const workflowName = expected.workflowName ?? CANDIDATE_WORKFLOW_NAME;
  if (
    run.workflowName !== undefined && run.workflowName !== workflowName
  ) {
    throw new Error(
      `candidate run workflow ${
        run.workflowName ?? "<missing>"
      } is not ${workflowName}`,
    );
  }
  if (
    expected.workflowPath !== undefined && run.workflowPath !== undefined &&
    run.workflowPath !== expected.workflowPath
  ) {
    throw new Error(
      `candidate run workflow path ${run.workflowPath} does not match ${expected.workflowPath}`,
    );
  }
  if (run.headSha !== undefined && run.headSha !== expected.sourceSha) {
    throw new Error(
      `candidate run source ${run.headSha} does not match expected ${expected.sourceSha}`,
    );
  }
  if (run.status !== undefined && run.status !== "completed") {
    throw new Error(
      `candidate run status ${run.status} is not completed`,
    );
  }
  if (run.conclusion !== undefined && run.conclusion !== "success") {
    throw new Error(
      `candidate run conclusion ${run.conclusion} is not success`,
    );
  }
  if (!run.headSha && !expected.sourceSha) {
    throw new Error("candidate run source SHA is required");
  }
}

/**
 * Build a single machine-readable cross-artifact ledger for one tag: CLI
 * archive, npm installer, Helm archive, and versioned container subjects with
 * their digests, bound to the tag and source commit. Rerunnable from the exact
 * tag because every subject is derived from immutable published identities.
 */
export function buildCrossArtifactLedger(input: {
  tag: string;
  version: string;
  sourceSha: string;
  candidateId: string;
  cliArchive: { name: string; sha256: string; size: number };
  npmTarball: { spec: string; sha256: string };
  helmArchive: { name: string; sha256: string; size: number };
  image: { repository: string; digest: string };
}): CrossArtifactLedger {
  if (!/^v\d+\.\d+\.\d+$/.test(input.tag)) {
    throw new Error(
      `ledger tag must be a stable release tag, got ${input.tag}`,
    );
  }
  if (!/^[0-9a-f]{40}$/.test(input.sourceSha)) {
    throw new Error("ledger source SHA must be a 40-character Git commit SHA");
  }
  return {
    schema_version: 1,
    tag: input.tag,
    version: input.version,
    source_sha: input.sourceSha,
    candidate_id: input.candidateId,
    entries: [
      {
        surface: "cli",
        digest: input.cliArchive.sha256,
        subject: input.cliArchive.name,
        size: input.cliArchive.size,
      },
      {
        surface: "npm",
        digest: input.npmTarball.sha256,
        subject: input.npmTarball.spec,
      },
      {
        surface: "helm",
        digest: input.helmArchive.sha256,
        subject: input.helmArchive.name,
        size: input.helmArchive.size,
      },
      {
        surface: "image",
        digest: input.image.digest,
        subject: `${input.image.repository}:${input.version}`,
      },
    ],
    helm_oci_note:
      "Helm archive digest is the .tgz file SHA-256; the OCI registry descriptor digest addresses the same chart version over the registry transport. Compare pulled bytes, never equate the digest strings.",
  };
}

/** Verify a ledger against freshly observed published digests/subjects. */
export function verifyCrossArtifactLedger(
  ledger: CrossArtifactLedger,
  observed: Record<string, { digest: string; subject: string }>,
): void {
  if (ledger.schema_version !== 1) {
    throw new Error("ledger schema_version must be 1");
  }
  for (const entry of ledger.entries) {
    const seen = observed[entry.surface];
    if (!seen) {
      throw new Error(`ledger is missing observed ${entry.surface} subject`);
    }
    if (seen.digest !== entry.digest) {
      throw new Error(
        `ledger ${entry.surface} digest ${seen.digest} differs from ${entry.digest}`,
      );
    }
    if (seen.subject !== entry.subject) {
      throw new Error(
        `ledger ${entry.surface} subject ${seen.subject} differs from ${entry.subject}`,
      );
    }
  }
}

async function readReleaseManifest(
  path: string,
): Promise<PublishedReleaseManifest> {
  const at = path || "<missing --manifest>";
  return parsePublishedReleaseManifest(
    JSON.parse(await Deno.readTextFile(at)),
  );
}

async function manifestFieldDigest(
  manifest: PublishedReleaseManifest,
  field: string,
): Promise<string> {
  switch (field) {
    case "npm":
      return manifest.npm_package.digest;
    case "helm":
      return manifest.helm_chart.digest;
    case "image":
      return manifest.image.digest;
    default:
      throw new Error(
        `unknown manifest field ${field}; expected npm|helm|image`,
      );
  }
}

async function readAssetNameList(from: string): Promise<string[]> {
  if (!from) return [];
  try {
    const stat = await Deno.stat(from);
    if (stat.isFile) {
      return (await Deno.readTextFile(from)).split("\n").map((line) =>
        line.trim()
      ).filter(Boolean);
    }
  } catch {
    // Fall through to comma-separated treatment.
  }
  return from.split(/[,\n]/).map((name) => name.trim()).filter(Boolean);
}

function flagValue(args: string[], flag: string): string | undefined {
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] : undefined;
}

function requiredFlag(args: string[], flag: string): string {
  const value = flagValue(args, flag);
  if (!value) {
    throw new Error(`distribution validation failed: ${flag} is required`);
  }
  return value;
}
