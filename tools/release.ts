import {
  candidateIdFromManifestBytes,
  type CandidateManifest,
  createVerificationReceipt,
  parseCandidateManifest,
  parseVerificationReceipt,
  RELEASE_SMOKE_POLICY,
  validateVerificationReceipt,
  type VerificationReceipt,
} from "./release_verify.ts";

const decoder = new TextDecoder();
const repoRoot = decodeURIComponent(new URL("../", import.meta.url).pathname)
  .replace(/\/$/, "");

type Version = { major: number; minor: number; patch: number };

type VersionState = {
  workspace: string;
  npmPackage: string;
  helmChart: string;
  helmApp: string;
  helmImageTag: string;
  versionFile: string;
  npmPackageName: string;
  npmRegistry: string;
};

type VerifiedCandidate = {
  manifestPath: string;
  manifest: CandidateManifest;
  candidateId: string;
};

if (import.meta.main) await main();

async function main(): Promise<void> {
  const [command, ...args] = Deno.args;
  switch (command) {
    case "version-sync":
      await synchronizeVersion((await readVersionState()).versionFile);
      break;
    case "version-check":
      await validateVersion();
      break;
    case "prepare":
      await prepareVersion(args[0]);
      break;
    case "candidate":
      await createCandidate();
      break;
    case "verify-candidate":
      await verifyCandidate(candidateManifestPath(args), args);
      break;
    case "verify-candidate-assets":
      await verifyCandidateAssets(
        await verifyCandidate(candidateManifestPath(args), args, true),
      );
      break;
    case "verify-candidate-smoke":
      await verifyCandidateSmoke(
        await verifyCandidate(candidateManifestPath(args), args, true),
      );
      break;
    case "write-verification-receipt":
      await writeVerificationReceipt(args);
      break;
    case "candidate-id":
      console.log(`sha256:${await sha256File(candidateManifestPath(args))}`);
      break;
    case "promote":
      {
        const candidate = await verifyCandidate(
          candidateManifestPath(args),
          args,
          true,
        );
        await readVerificationReceipt(candidate, args);
        await promote(candidate, verificationReceiptPath(candidate, args));
      }
      break;
    case "promote-aliases":
      await promoteAliases(
        await verifyCandidate(candidateManifestPath(args), args, true),
      );
      break;
    case "package-cli":
      await packageCli();
      break;
    case "verify-cli":
      await verifyCli();
      break;
    case "package-npm":
      await packageNpm();
      break;
    case "package-helm":
      await packageHelm();
      break;
    case "verify-npm":
      await verifyNpm();
      break;
    case "verify-helm":
      await verifyHelmPackage();
      break;
    default:
      throw new Error(
        "usage: deno run -A tools/release.ts <version-sync|version-check|prepare compatible|prepare breaking|candidate|verify-candidate|verify-candidate-smoke|verify-candidate-assets|write-verification-receipt|candidate-id|promote|promote-aliases|package-cli|verify-cli|package-npm|package-helm|verify-npm|verify-helm>",
      );
  }
}

async function validateVersion(): Promise<VersionState> {
  const state = await readVersionState();
  const canonical = parseStableVersion(state.versionFile);
  const projections = [
    ["Cargo workspace", state.workspace],
    ["npm package", state.npmPackage],
    ["Helm chart", state.helmChart],
    ["Helm appVersion", state.helmApp],
    ["Helm image tag", state.helmImageTag],
  ] as const;
  for (const [label, value] of projections) {
    if (value !== state.versionFile) {
      throw new Error(
        `${label} version ${value} does not match version.txt ${state.versionFile}`,
      );
    }
  }
  if (state.npmPackageName !== "@ugoite/ugoite") {
    throw new Error(
      `packages/ugoite/package.json name must be @ugoite/ugoite, got ${state.npmPackageName}`,
    );
  }
  if (state.npmRegistry !== "https://npm.pkg.github.com") {
    throw new Error(
      `packages/ugoite/package.json publishConfig.registry must be https://npm.pkg.github.com, got ${state.npmRegistry}`,
    );
  }

  const cargoLock = await readText("Cargo.lock");
  const workspaceNames = await workspacePackageNames();
  for (const name of workspaceNames) {
    const match = cargoLock.match(
      new RegExp(`name = "${escapeRegExp(name)}"\\nversion = "([^"]+)"`),
    );
    if (!match) {
      throw new Error(`Cargo.lock is missing workspace package ${name}`);
    }
    if (match[1] !== state.versionFile) {
      throw new Error(
        `Cargo.lock package ${name} version ${
          match[1]
        } does not match ${state.versionFile}`,
      );
    }
  }

  await run("cargo", [
    "metadata",
    "--locked",
    "--format-version",
    "1",
    "--no-deps",
  ]);
  if (canonical.major < 0) throw new Error("invalid canonical version");
  return state;
}

async function synchronizeVersion(version: string): Promise<void> {
  parseStableVersion(version);
  const cargoPath = pathJoin("Cargo.toml");
  const cargo = await Deno.readTextFile(cargoPath);
  const workspaceMatch = cargo.match(
    /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*")([^"]+)(")/,
  );
  if (!workspaceMatch) {
    throw new Error("workspace version was not found in Cargo.toml");
  }
  const nextCargo = cargo.replace(
    workspaceMatch[0],
    `${workspaceMatch[1]}${version}${workspaceMatch[3]}`,
  );
  if (nextCargo !== cargo) await Deno.writeTextFile(cargoPath, nextCargo);

  const packagePath = pathJoin("packages", "ugoite", "package.json");
  const packageJson = JSON.parse(
    await Deno.readTextFile(packagePath),
  ) as Record<
    string,
    unknown
  >;
  packageJson.version = version;
  await Deno.writeTextFile(
    packagePath,
    `${JSON.stringify(packageJson, null, 2)}\n`,
  );

  await replaceLine(
    "charts/ugoite/Chart.yaml",
    /^version:\s*[^\n]+$/m,
    `version: ${version}`,
  );
  await replaceLine(
    "charts/ugoite/Chart.yaml",
    /^appVersion:\s*[^\n]+$/m,
    `appVersion: "${version}"`,
  );
  await replaceLine(
    "charts/ugoite/values.yaml",
    /^\x20\x20tag:\s*[^\n]+$/m,
    `  tag: ${version}`,
  );

  // Cargo owns the lockfile projection. Resolve only after the workspace
  // package version has changed; this non-locked standard Cargo operation
  // updates only the generated workspace package entries in the existing
  // lockfile. The locked validation below prevents dependency drift in all
  // other release paths.
  await run("cargo", ["check", "--workspace"]);
  await validateVersion();
}

async function prepareVersion(change: string | undefined): Promise<void> {
  if (change !== "compatible" && change !== "breaking") {
    throw new Error(
      "release preparation requires exactly compatible or breaking",
    );
  }
  const state = await validateVersion();
  const latest = await latestPublishedStableVersion();
  if (!latest) {
    throw new Error(
      "no published stable version was found; the prepared first release must be promoted as-is",
    );
  }
  const prepared = parseStableVersion(state.versionFile);
  const comparison = compareVersions(prepared, latest);
  if (comparison > 0) {
    throw new Error(`${state.versionFile} is already prepared`);
  }
  if (comparison < 0) {
    throw new Error(
      `prepared version ${state.versionFile} is behind latest published stable ${
        formatVersion(latest)
      }`,
    );
  }
  if (latest.major !== 0) {
    throw new Error(
      "release:prepare compatible|breaking is pre-1.0 only; define the stable SemVer preparation contract before using it after 1.0",
    );
  }
  const next = change === "compatible"
    ? { ...latest, patch: latest.patch + 1 }
    : { major: latest.major, minor: latest.minor + 1, patch: 0 };
  await synchronizeVersion(formatVersion(next));
  console.log(`prepared ${state.versionFile} -> ${formatVersion(next)}`);
}

async function latestPublishedStableVersion(): Promise<Version | null> {
  const tags = (await run("git", ["tag", "--list", "v*"])).stdout
    .split("\n")
    .map((tag) => tag.trim())
    .filter(Boolean);
  const versions = tags.flatMap((tag) => {
    const match = /^v(\d+)\.(\d+)\.(\d+)$/.exec(tag);
    return match
      ? [{
        major: Number(match[1]),
        minor: Number(match[2]),
        patch: Number(match[3]),
      }]
      : [];
  });
  return versions.sort(compareVersions).at(-1) ?? null;
}

/**
 * Run the stable cross-surface acceptance corpus before a candidate may be
 * recorded. Only executed, non-Playwright evidence qualifies here: the
 * capability projection integrity check, the CLI core and remote journey
 * plus parity harnesses, and the Mitase specification graph check. The
 * Playwright journey stays on the `full` E2E lane until the v0.2 closure.
 * Any failure refuses candidate creation without touching artifacts.
 */
async function qualifyAcceptanceCorpus(): Promise<void> {
  console.log("qualifying cross-surface acceptance corpus");
  await run("deno", ["run", "-A", "tools/capability_report.ts", "--json"]);
  await run("cargo", [
    "test",
    "-p",
    "ugoite-cli",
    "--test",
    "test_journey_core",
    "--test",
    "test_journey_remote",
    "--locked",
  ]);
  await run("./scripts/mitase", ["check", "."]);
}

async function createCandidate(): Promise<void> {
  await qualifyAcceptanceCorpus();
  if (Deno.env.get("UGOITE_RELEASE_CANDIDATE_PREBUILT") !== "true") {
    await run("mise", ["run", "build"]);
    await run("mise", ["run", "package"]);
    await run("mise", ["run", "verify"]);
    await run("mise", ["run", "package:npm"]);
    await run("mise", ["run", "verify:npm"]);
    await stageReleaseAssets();
  }
  await validateVersion();
  await run("deno", [
    "run",
    "-A",
    "tools/artifacts.ts",
    "write-candidate-manifest",
  ]);
  const manifestPath = candidateManifestPath([]);
  await verifyCandidate(manifestPath);
  console.log(`candidate_manifest=${manifestPath}`);
  console.log(`candidate_id=sha256:${await sha256File(manifestPath)}`);
}

async function verifyCandidate(
  manifestPath: string,
  args: string[] = [],
  requireCandidateRunId = false,
): Promise<VerifiedCandidate> {
  await ensureFile(manifestPath, "candidate manifest");
  const bytes = await Deno.readFile(manifestPath);
  const manifest = parseCandidateManifest(bytes);
  const candidateId = await candidateIdFromManifestBytes(bytes);
  const expectedRunId = flagValue(args, "--candidate-run-id") ??
    Deno.env.get("UGOITE_CANDIDATE_RUN_ID");
  if (requireCandidateRunId && !expectedRunId) {
    throw new Error(
      "candidate run ID is required for publication verification",
    );
  }
  if (expectedRunId && expectedRunId !== manifest.ci_run_id) {
    throw new Error(
      `candidate run ID ${manifest.ci_run_id} does not match requested ${expectedRunId}`,
    );
  }
  await run("git", ["cat-file", "-e", `${manifest.source_sha}^{commit}`]);
  const sourceVersion =
    (await run("git", ["show", `${manifest.source_sha}:version.txt`])).stdout
      .trim();
  if (sourceVersion !== manifest.version) {
    throw new Error(
      `candidate version ${manifest.version} does not match version.txt at ${manifest.source_sha}: ${sourceVersion}`,
    );
  }

  const manifestDirectory = dirname(manifestPath);
  const kinds = new Set<string>();
  for (const artifact of manifest.artifacts ?? []) {
    kinds.add(artifact.kind);
    if (
      artifact.kind !== "image" &&
      (!artifact.files || artifact.files.length === 0)
    ) {
      throw new Error(`candidate ${artifact.kind} artifact has no files`);
    }
    for (const file of artifact.files ?? []) {
      const filePath = safeCandidatePath(manifestDirectory, file.path);
      await ensureFile(filePath, `candidate artifact ${file.path}`);
      const digest = await sha256File(filePath);
      if (digest !== file.sha256) {
        throw new Error(`candidate digest mismatch for ${file.path}`);
      }
      if ((await Deno.stat(filePath)).size !== file.size) {
        throw new Error(`candidate size mismatch for ${file.path}`);
      }
    }
    if (artifact.kind === "image") {
      const digest = artifact.config?.digest ?? "";
      if (!/^sha256:[0-9a-f]{64}$/.test(digest)) {
        throw new Error(
          "candidate container artifact must record an OCI digest",
        );
      }
    }
  }
  const release = manifest.artifacts.find((artifact) =>
    artifact.kind === "release"
  );
  const compose = release?.files.find((file) =>
    file.path.endsWith("docker-compose.release.yaml")
  );
  const composeChecksum = release?.files.find((file) =>
    file.path.endsWith("docker-compose.release.yaml.sha256")
  );
  if (!compose || !composeChecksum) {
    throw new Error(
      "candidate release artifact must include Compose and its checksum",
    );
  }
  await verifyChecksumFile(
    safeCandidatePath(manifestDirectory, compose.path),
    safeCandidatePath(manifestDirectory, composeChecksum.path),
  );
  for (const required of ["cli", "npm", "helm", "image", "release"]) {
    if (!kinds.has(required)) {
      throw new Error(`candidate manifest is missing ${required} artifact`);
    }
  }
  console.log(
    `verified candidate ${candidateId} (${manifest.version}, ${manifest.source_sha})`,
  );
  return { manifestPath, manifest, candidateId };
}

async function verifyCandidateSmoke(
  candidate: VerifiedCandidate,
): Promise<void> {
  await verifyCandidateCliArchive(candidate);
  await verifyCandidateContainer(candidate);
  console.log(
    `candidate smoke verification passed for ${candidate.manifest.version}`,
  );
}

async function verifyCandidateCliArchive(
  candidate: VerifiedCandidate,
): Promise<void> {
  const artifact =
    candidate.manifest.artifacts.find((entry) =>
      entry.kind === "cli" &&
      entry.config?.platform === "x86_64-unknown-linux-gnu"
    ) ?? candidate.manifest.artifacts.find((entry) => entry.kind === "cli");
  const archive = artifact?.files.find((file) => file.path.endsWith(".tar.gz"));
  if (!archive) throw new Error("candidate CLI archive is missing");
  const archivePath = safeCandidatePath(
    dirname(candidate.manifestPath),
    archive.path,
  );
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-candidate-cli-" });
  const workspace = await Deno.makeTempDir({
    prefix: "ugoite-candidate-space-",
  });
  const configPath = pathJoin(workspace, "cli-config.json");
  const previousConfigPath = Deno.env.get("UGOITE_CLI_CONFIG_PATH");
  Deno.env.set("UGOITE_CLI_CONFIG_PATH", configPath);
  try {
    await run("tar", ["-xzf", archivePath, "-C", tempDir]);
    const binary = pathJoin(tempDir, "ugoite");
    const versionOutput = await run(binary, ["--version"]);
    if (!versionOutput.stdout.includes(candidate.manifest.version)) {
      throw new Error(
        `candidate CLI reported ${versionOutput.stdout}, expected ${candidate.manifest.version}`,
      );
    }
    const spaceRoot = pathJoin(workspace, "spaces");
    const listBefore = JSON.parse(
      (await run(binary, ["space", "list", workspace])).stdout,
    ) as unknown;
    if (!Array.isArray(listBefore) || listBefore.length !== 0) {
      throw new Error("candidate CLI initial Space list was not empty");
    }
    const create = JSON.parse(
      (await run(binary, ["space", "create", pathJoin(spaceRoot, "smoke")]))
        .stdout,
    ) as { created?: boolean; slug?: string; id?: string };
    if (create.created !== true || create.slug !== "smoke" || !create.id) {
      throw new Error(
        `candidate CLI Space create returned ${JSON.stringify(create)}`,
      );
    }
    const listAfter = JSON.parse(
      (await run(binary, ["space", "list", workspace])).stdout,
    ) as unknown;
    if (!Array.isArray(listAfter) || !listAfter.includes(create.id)) {
      throw new Error(
        "candidate CLI Space list did not contain the created Space",
      );
    }
  } finally {
    if (previousConfigPath === undefined) {
      Deno.env.delete("UGOITE_CLI_CONFIG_PATH");
    } else Deno.env.set("UGOITE_CLI_CONFIG_PATH", previousConfigPath);
    await Deno.remove(tempDir, { recursive: true }).catch(() => {});
    await Deno.remove(workspace, { recursive: true }).catch(() => {});
  }
}

async function verifyCandidateContainer(
  candidate: VerifiedCandidate,
): Promise<void> {
  const image = candidate.manifest.artifacts.find((entry) =>
    entry.kind === "image"
  );
  const repository = image?.config?.repository;
  const digest = image?.config?.digest;
  if (!repository || !digest) {
    throw new Error("candidate container coordinates are incomplete");
  }
  const imageRef = `${repository}@${digest}`;
  const name = `ugoite-candidate-smoke-${crypto.randomUUID()}`;
  const secret = [...crypto.getRandomValues(new Uint8Array(32))].map((byte) =>
    byte.toString(16).padStart(2, "0")
  ).join("");
  let containerId = "";
  try {
    containerId = (await run("docker", [
      "run",
      "--detach",
      "--pull",
      "always",
      "--name",
      name,
      "--publish",
      "127.0.0.1::8000",
      "--env",
      "UGOITE_ROOT=/data",
      "--env",
      "UGOITE_SERVER_ADDRESS=0.0.0.0:8000",
      "--env",
      "UGOITE_STATIC_DIR=/app/static",
      "--env",
      "UGOITE_PUBLIC_ORIGIN=http://localhost",
      "--env",
      "UGOITE_WEBAUTHN_RP_ID=localhost",
      "--env",
      `UGOITE_NODE_SECRET_KEY=${secret}`,
      imageRef,
    ])).stdout;
    const port = (await run("docker", ["port", containerId, "8000/tcp"])).stdout
      .trim().split(":").at(-1);
    if (!port) throw new Error("candidate container did not expose port 8000");
    const healthUrl = `http://127.0.0.1:${port}/health`;
    for (let attempt = 0; attempt < 60; attempt++) {
      const health = await tryRun("curl", ["-fsS", healthUrl]);
      if (health.success) return;
      await new Promise((resolve) => setTimeout(resolve, 2_000));
    }
    throw new Error(`candidate container health check failed: ${healthUrl}`);
  } finally {
    if (containerId) await tryRun("docker", ["rm", "--force", containerId]);
    else await tryRun("docker", ["rm", "--force", name]);
  }
}

async function writeVerificationReceipt(args: string[]): Promise<void> {
  const manifestPath = candidateManifestPath(args);
  const candidateRunId = flagValue(args, "--candidate-run-id") ??
    Deno.env.get("UGOITE_CANDIDATE_RUN_ID");
  const verifierWorkflowSha = flagValue(args, "--verifier-workflow-sha") ??
    Deno.env.get("UGOITE_VERIFIER_WORKFLOW_SHA");
  const verificationRunId = flagValue(args, "--verification-run-id") ??
    Deno.env.get("GITHUB_RUN_ID");
  const policy = flagValue(args, "--policy") ?? RELEASE_SMOKE_POLICY;
  if (!candidateRunId) throw new Error("candidate run ID is required");
  if (!verifierWorkflowSha) {
    throw new Error("verifier workflow SHA is required");
  }
  if (!verificationRunId) throw new Error("verification run ID is required");
  const candidate = await verifyCandidate(manifestPath, [
    "--candidate-run-id",
    candidateRunId,
  ], true);
  const receipt = createVerificationReceipt({
    candidateId: candidate.candidateId,
    candidateRunId,
    verifierWorkflowSha,
    verificationRunId,
    policy,
  });
  const outputPath = flagValue(args, "--output") ??
    pathJoin(dirname(manifestPath), "verification-receipt.json");
  await Deno.writeTextFile(outputPath, `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(`verification_receipt=${outputPath}`);
}

async function readVerificationReceipt(
  candidate: VerifiedCandidate,
  args: string[] = [],
): Promise<VerificationReceipt> {
  const receiptPath = verificationReceiptPath(candidate, args);
  await ensureFile(receiptPath, "verification receipt");
  let value: unknown;
  try {
    value = JSON.parse(await Deno.readTextFile(receiptPath));
  } catch (error) {
    throw new Error(
      `verification receipt is not valid JSON: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
  }
  const receipt = parseVerificationReceipt(value);
  validateVerificationReceipt(receipt, {
    candidateId: candidate.candidateId,
    candidateRunId: candidate.manifest.ci_run_id,
    policy: RELEASE_SMOKE_POLICY,
  });
  return receipt;
}

async function verifyCandidateAssets(
  candidate: VerifiedCandidate,
): Promise<void> {
  const directory = dirname(candidate.manifestPath);
  await verifyCandidateCliArchives(candidate, directory);
  await verifyCandidateCliArchive(candidate);
  await verifyCandidateNpmArchive(candidate, directory);
  await verifyCandidateHelmArchive(candidate, directory);
  await verifyCandidateImage(candidate);
  console.log(
    `verified candidate assets directly for ${candidate.candidateId}`,
  );
}

async function verifyCandidateCliArchives(
  candidate: VerifiedCandidate,
  directory: string,
): Promise<void> {
  const hostTarget = Deno.build.os === "darwin"
    ? `${Deno.build.arch === "aarch64" ? "aarch64" : "x86_64"}-apple-darwin`
    : `${
      Deno.build.arch === "aarch64" ? "aarch64" : "x86_64"
    }-unknown-linux-gnu`;
  let executed = false;
  for (
    const artifact of candidate.manifest.artifacts.filter((entry) =>
      entry.kind === "cli"
    )
  ) {
    const artifactPlatform = artifact.config?.platform ?? hostTarget;
    const archive = artifact.files.find((file) =>
      file.path.endsWith(".tar.gz")
    );
    if (!archive) {
      throw new Error(
        `candidate CLI archive is missing for ${artifactPlatform}`,
      );
    }
    const tempDir = await Deno.makeTempDir({ prefix: "ugoite-candidate-cli-" });
    try {
      await run("tar", [
        "-xzf",
        safeCandidatePath(directory, archive.path),
        "-C",
        tempDir,
      ]);
      if (artifactPlatform === hostTarget) {
        const output = await run(pathJoin(tempDir, "ugoite"), ["--version"]);
        if (!output.stdout.includes(candidate.manifest.version)) {
          throw new Error(
            `candidate CLI ${artifactPlatform} did not report ${candidate.manifest.version}`,
          );
        }
        executed = true;
      }
    } finally {
      await Deno.remove(tempDir, { recursive: true }).catch(() => {});
    }
  }
  if (!executed) {
    throw new Error(
      `candidate CLI has no archive for runner target ${hostTarget}`,
    );
  }
}

async function verifyCandidateNpmArchive(
  candidate: VerifiedCandidate,
  directory: string,
): Promise<void> {
  const artifact = candidate.manifest.artifacts.find((entry) =>
    entry.kind === "npm"
  );
  const archive = artifact?.files.find((file) => file.path.endsWith(".tgz"));
  if (!archive) throw new Error("candidate npm tarball is missing");
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-candidate-npm-" });
  try {
    await run("npm", [
      "install",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      "--prefix",
      tempDir,
      safeCandidatePath(directory, archive.path),
    ]);
    const packageJson = JSON.parse(
      await Deno.readTextFile(
        pathJoin(tempDir, "node_modules", "@ugoite", "ugoite", "package.json"),
      ),
    ) as { name?: string; version?: string };
    if (
      packageJson.name !== "@ugoite/ugoite" ||
      packageJson.version !== candidate.manifest.version
    ) {
      throw new Error(
        "candidate npm tarball metadata does not match candidate",
      );
    }
  } finally {
    await Deno.remove(tempDir, { recursive: true }).catch(() => {});
  }
}

async function verifyCandidateHelmArchive(
  candidate: VerifiedCandidate,
  directory: string,
): Promise<void> {
  const artifact = candidate.manifest.artifacts.find((entry) =>
    entry.kind === "helm"
  );
  const archive = artifact?.files.find((file) => file.path.endsWith(".tgz"));
  if (!archive) throw new Error("candidate Helm archive is missing");
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-candidate-helm-" });
  try {
    await run("tar", [
      "-xzf",
      safeCandidatePath(directory, archive.path),
      "-C",
      tempDir,
    ]);
    await run("helm", ["lint", pathJoin(tempDir, "ugoite")]);
  } finally {
    await Deno.remove(tempDir, { recursive: true }).catch(() => {});
  }
}

async function verifyCandidateImage(
  candidate: VerifiedCandidate,
): Promise<void> {
  const artifact = candidate.manifest.artifacts.find((entry) =>
    entry.kind === "image"
  );
  const config = artifact?.config ?? {};
  const repository = config.repository;
  const expectedDigest = config.digest;
  if (!repository || !expectedDigest) {
    throw new Error("candidate container coordinates are incomplete");
  }
  const exactRef = `${repository}@${expectedDigest}`;
  const inspect = await run("docker", [
    "buildx",
    "imagetools",
    "inspect",
    exactRef,
    "--format",
    "{{json .Manifest.Digest}}",
  ]);
  if (inspect.stdout.replaceAll('"', "").trim() !== expectedDigest) {
    throw new Error(`candidate container digest is not ${expectedDigest}`);
  }
  const containerName = `ugoite-candidate-${crypto.randomUUID()}`;
  const nodeSecret = btoa(
    String.fromCharCode(...crypto.getRandomValues(new Uint8Array(32))),
  );
  await run("docker", [
    "run",
    "--detach",
    "--rm",
    "--name",
    containerName,
    "--publish",
    "127.0.0.1::8000",
    "--env",
    "UGOITE_ROOT=/data",
    "--env",
    "UGOITE_SERVER_ADDRESS=0.0.0.0:8000",
    "--env",
    "UGOITE_PUBLIC_ORIGIN=http://localhost",
    "--env",
    "UGOITE_API_BASE_URL=http://localhost/api",
    "--env",
    "UGOITE_WEBAUTHN_RP_ID=localhost",
    "--env",
    `UGOITE_NODE_SECRET_KEY=${nodeSecret}`,
    exactRef,
  ]);
  try {
    const port = (await run("docker", ["port", containerName, "8000/tcp"]))
      .stdout.match(/:(\d+)\s*$/)?.[1];
    if (!port) throw new Error("candidate container did not publish port 8000");
    let healthy = false;
    for (let attempt = 0; attempt < 60; attempt++) {
      try {
        const response = await fetch(`http://127.0.0.1:${port}/health`);
        if (response.ok) {
          healthy = true;
          break;
        }
      } catch {
        // The released server may still be starting.
      }
      await new Promise((resolve) => setTimeout(resolve, 1_000));
    }
    if (!healthy) throw new Error("candidate container did not become healthy");
  } finally {
    await tryRun("docker", ["rm", "--force", containerName]);
  }
}

async function stageReleaseAssets(): Promise<void> {
  const source = pathJoin("docker-compose.release.yaml");
  const destination = pathJoin("target", "artifacts", basename(source));
  await ensureFile(source, "release compose file");
  await Deno.mkdir(dirname(destination), { recursive: true });
  if (!(await isSameFile(source, destination))) {
    await Deno.copyFile(source, destination);
  }
  await writeChecksumFile(destination, `${destination}.sha256`);
}

async function isSameFile(
  leftPath: string,
  rightPath: string,
): Promise<boolean> {
  try {
    return bytesEqual(
      await Deno.readFile(leftPath),
      await Deno.readFile(rightPath),
    );
  } catch {
    return false;
  }
}

async function promote(
  candidate: VerifiedCandidate,
  receiptPath: string,
): Promise<void> {
  if (Deno.env.get("UGOITE_PROMOTION_DRY_RUN") === "true") {
    console.log(
      `dry-run promotion ${candidate.candidateId} for v${candidate.manifest.version}`,
    );
    return;
  }
  const version = candidate.manifest.version;
  const stableTag = `v${version}`;
  const draftTag = candidateDraftTag(candidate);
  const releaseAssets = await prepareReleaseAssets(candidate, receiptPath);
  const releaseFiles = [
    candidate.manifestPath,
    ...releaseAssets,
  ];
  await ensureDraftRelease(draftTag, candidate.manifest.source_sha);
  await publishReleaseFiles(draftTag, releaseFiles);
  await publishNpm(candidate);
  await publishHelm(candidate);
  await publishContainer(candidate);
  await ensureStableRelease(
    stableTag,
    candidate.manifest.source_sha,
    releaseFiles,
  );
  console.log(`promoted ${candidate.candidateId} as ${stableTag}`);
}

async function promoteAliases(candidate: VerifiedCandidate): Promise<void> {
  const image = candidate.manifest.artifacts.find((artifact) =>
    artifact.kind === "image"
  );
  const config = image?.config ?? {};
  const repository = config.repository;
  if (!repository || !config.tag) {
    throw new Error("candidate image repository and tag are required");
  }
  await run("docker", [
    "buildx",
    "imagetools",
    "create",
    "--tag",
    `${repository}:latest`,
    `${repository}:${candidate.manifest.version}`,
  ]);
  await run("npm", [
    "dist-tag",
    "add",
    `@ugoite/ugoite@${candidate.manifest.version}`,
    "latest",
  ]);
  console.log(`updated mutable aliases from ${repository}`);
}

async function ensureDraftRelease(
  tag: string,
  sourceSha: string,
): Promise<void> {
  const existing = await tryRun("gh", [
    "release",
    "view",
    tag,
    "--json",
    "tagName,targetCommitish",
  ]);
  if (existing.success) {
    const release = JSON.parse(existing.stdout) as {
      tagName?: string;
      targetCommitish?: string;
    };
    if (release.tagName !== tag) {
      throw new Error(`GitHub Release tag mismatch for ${tag}`);
    }
    if (release.targetCommitish && release.targetCommitish !== sourceSha) {
      const resolved = await tryRun("git", [
        "rev-parse",
        `${release.targetCommitish}^{commit}`,
      ]);
      if (!resolved.success || resolved.stdout.trim() !== sourceSha) {
        throw new Error(
          `GitHub Release ${tag} does not target candidate source ${sourceSha}`,
        );
      }
    }
    return;
  }
  if (!isMissing(existing.stderr)) throw new Error(existing.stderr);
  await run("gh", [
    "release",
    "create",
    tag,
    "--draft",
    "--title",
    `Ugoite ${tag}`,
    "--target",
    sourceSha,
    "--notes",
    "Verified Ugoite release candidate.",
  ]);
}

async function ensureStableRelease(
  tag: string,
  sourceSha: string,
  releaseFiles: string[],
): Promise<void> {
  let filesToVerify = releaseFiles;
  let alreadyImmutable = false;
  const existing = await tryRun("gh", [
    "release",
    "view",
    tag,
    "--json",
    "tagName,targetCommitish,isDraft,isImmutable",
  ]);
  if (existing.success) {
    const release = JSON.parse(existing.stdout) as {
      tagName?: string;
      targetCommitish?: string;
      isDraft?: boolean;
      isImmutable?: boolean;
    };
    if (release.tagName !== tag) {
      throw new Error(`GitHub Release tag mismatch for ${tag}`);
    }
    if (release.isDraft === false && release.isImmutable !== true) {
      throw new Error(
        `GitHub Release ${tag} is published but not immutable`,
      );
    }
    if (release.targetCommitish && release.targetCommitish !== sourceSha) {
      const resolved = await tryRun("git", [
        "rev-parse",
        `${release.targetCommitish}^{commit}`,
      ]);
      if (!resolved.success || resolved.stdout.trim() !== sourceSha) {
        throw new Error(
          `GitHub Release ${tag} does not target candidate source ${sourceSha}`,
        );
      }
    }
    if (release.isImmutable) {
      alreadyImmutable = true;
      filesToVerify = releaseFiles.filter((filePath) =>
        !basename(filePath).startsWith("verification-receipt-")
      );
    } else {
      await publishReleaseFiles(tag, releaseFiles);
    }
  } else {
    if (!isMissing(existing.stderr)) throw new Error(existing.stderr);
    await run("gh", [
      "release",
      "create",
      tag,
      "--draft",
      "--title",
      `Ugoite ${tag}`,
      "--target",
      sourceSha,
      "--notes",
      "Verified Ugoite release candidate.",
      ...releaseFiles,
    ]);
  }
  for (const filePath of filesToVerify) {
    await verifyPublishedReleaseFile(tag, filePath);
  }
  if (alreadyImmutable) return;
  await run("gh", ["release", "edit", tag, "--draft=false"]);
  const published = JSON.parse(
    (await run("gh", [
      "release",
      "view",
      tag,
      "--json",
      "isDraft,isImmutable",
    ])).stdout,
  ) as { isDraft?: boolean; isImmutable?: boolean };
  if (published.isDraft !== false || published.isImmutable !== true) {
    throw new Error(
      `GitHub Release ${tag} was not published as an immutable release`,
    );
  }
}

async function prepareReleaseAssets(
  candidate: VerifiedCandidate,
  receiptPath: string,
): Promise<string[]> {
  const directory = dirname(candidate.manifestPath);
  const idPath = pathJoin(directory, "candidate-id.txt");
  const publicManifestPath = pathJoin(directory, "release-manifest.json");
  const image = candidate.manifest.artifacts.find((artifact) =>
    artifact.kind === "image"
  );
  const npm = candidate.manifest.artifacts.find((artifact) =>
    artifact.kind === "npm"
  );
  const helm = candidate.manifest.artifacts.find((artifact) =>
    artifact.kind === "helm"
  );
  const npmPackage = npm?.files.find((file) => file.path.endsWith(".tgz"));
  const helmChart = helm?.files.find((file) => file.path.endsWith(".tgz"));
  if (!npmPackage || !helmChart) {
    throw new Error("candidate npm and Helm packages are required");
  }
  const releaseAssets = candidate.manifest.artifacts
    .filter((artifact) =>
      artifact.kind === "cli" || artifact.kind === "release"
    )
    .flatMap((artifact) => artifact.files)
    .map((file) => safeCandidatePath(directory, file.path));
  await ensureFile(receiptPath, "verification receipt");
  const receipt = parseVerificationReceipt(
    JSON.parse(await Deno.readTextFile(receiptPath)),
  );
  validateVerificationReceipt(receipt, {
    candidateId: candidate.candidateId,
    candidateRunId: candidate.manifest.ci_run_id,
    policy: RELEASE_SMOKE_POLICY,
  });
  const publicManifest = {
    schema_version: 3,
    release_tag: `v${candidate.manifest.version}`,
    version: candidate.manifest.version,
    source_sha: candidate.manifest.source_sha,
    candidate_id: candidate.candidateId,
    files: candidate.manifest.artifacts
      .filter((artifact) =>
        artifact.kind === "cli" || artifact.kind === "release"
      )
      .flatMap((artifact) => artifact.files)
      .map((file) => ({
        name: basename(file.path),
        sha256: file.sha256,
        size: file.size,
      })),
    image: {
      repository: image?.config?.repository ?? "",
      digest: image?.config?.digest ?? "",
    },
    npm_package: {
      name: npm?.config?.package ?? "@ugoite/ugoite",
      digest: npmPackage.sha256,
    },
    helm_chart: {
      repository: "oci://ghcr.io/ugoite/charts/ugoite",
      digest: helmChart.sha256,
    },
  };
  await Deno.writeTextFile(
    publicManifestPath,
    `${JSON.stringify(publicManifest, null, 2)}\n`,
  );
  await Deno.writeTextFile(idPath, `${candidate.candidateId}\n`);
  return [
    publicManifestPath,
    idPath,
    receiptPath,
    ...releaseAssets,
  ];
}

function candidateDraftTag(candidate: VerifiedCandidate): string {
  const shortSource = candidate.manifest.source_sha.slice(0, 12);
  const shortCandidate = candidate.candidateId.slice(-12);
  return `candidate-${shortSource}-${shortCandidate}`;
}

function verificationReceiptPath(
  candidate: VerifiedCandidate,
  args: string[],
): string {
  return flagValue(args, "--verification-receipt") ??
    pathJoin(dirname(candidate.manifestPath), "verification-receipt.json");
}

async function publishReleaseFiles(
  tag: string,
  paths: string[],
): Promise<void> {
  const result = await run("gh", ["release", "view", tag, "--json", "assets"]);
  const assets =
    (JSON.parse(result.stdout) as { assets?: Array<{ name: string }> })
      .assets ?? [];
  const existing = new Set(assets.map((asset) => asset.name));
  for (const filePath of paths) {
    const name = basename(filePath);
    if (!existing.has(name)) {
      await run("gh", ["release", "upload", tag, filePath]);
    }
    await verifyPublishedReleaseFile(tag, filePath);
  }
}

async function verifyPublishedReleaseFile(
  tag: string,
  filePath: string,
): Promise<void> {
  const name = basename(filePath);
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-release-asset-" });
  try {
    await run("gh", [
      "release",
      "download",
      tag,
      "--pattern",
      name,
      "--dir",
      tempDir,
    ]);
    const current = await Deno.readFile(pathJoin(tempDir, name));
    const expected = await Deno.readFile(filePath);
    if (!bytesEqual(current, expected)) {
      throw new Error(
        `published release asset ${name} differs from candidate`,
      );
    }
    console.log(`release asset ${name} matches candidate`);
  } finally {
    await Deno.remove(tempDir, { recursive: true }).catch(() => {});
  }
}

async function publishNpm(candidate: VerifiedCandidate): Promise<void> {
  const artifact = candidate.manifest.artifacts.find((entry) =>
    entry.kind === "npm"
  );
  const tarball = artifact?.files.find((file) => file.path.endsWith(".tgz"));
  if (!tarball) throw new Error("candidate npm tarball is missing");
  const tarballPath = safeCandidatePath(
    dirname(candidate.manifestPath),
    tarball.path,
  );
  const packageName = "@ugoite/ugoite";
  const version = candidate.manifest.version;
  const existing = await tryRun("npm", [
    "view",
    `${packageName}@${version}`,
    "version",
  ]);
  if (!existing.success && !isMissing(existing.stderr)) {
    throw new Error(existing.stderr);
  }
  if (!existing.success) {
    await run("npm", [
      "publish",
      tarballPath,
      "--tag",
      `candidate-${candidate.candidateId.slice(-12)}`,
    ]);
  }
  await verifyPublishedNpm(packageName, version, tarballPath);
}

async function verifyPublishedNpm(
  packageName: string,
  version: string,
  tarballPath: string,
): Promise<void> {
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-npm-" });
  try {
    const tarballUrl = (await run("npm", [
      "view",
      `${packageName}@${version}`,
      "dist.tarball",
    ])).stdout;
    const published = pathJoin(tempDir, basename(tarballPath));
    const curlArgs = ["-fsSL"];
    const token = Deno.env.get("NODE_AUTH_TOKEN")?.trim();
    if (token) curlArgs.push("-H", `Authorization: Bearer ${token}`);
    curlArgs.push(tarballUrl, "-o", published);
    await run("curl", curlArgs);
    if (
      !bytesEqual(
        await Deno.readFile(tarballPath),
        await Deno.readFile(published),
      )
    ) {
      throw new Error(
        `published npm package ${packageName}@${version} differs from candidate`,
      );
    }
  } finally {
    await Deno.remove(tempDir, { recursive: true }).catch(() => {});
  }
  console.log(`npm ${packageName}@${version} matches candidate`);
}

async function publishHelm(candidate: VerifiedCandidate): Promise<void> {
  const artifact = candidate.manifest.artifacts.find((entry) =>
    entry.kind === "helm"
  );
  const archive = artifact?.files.find((file) => file.path.endsWith(".tgz"));
  if (!archive) throw new Error("candidate Helm archive is missing");
  const archivePath = safeCandidatePath(
    dirname(candidate.manifestPath),
    archive.path,
  );
  const chartRef = "oci://ghcr.io/ugoite/charts/ugoite";
  const existing = await tryRun("helm", [
    "show",
    "chart",
    chartRef,
    "--version",
    candidate.manifest.version,
  ]);
  if (!existing.success && !isMissing(existing.stderr)) {
    throw new Error(existing.stderr);
  }
  if (!existing.success) {
    await run("helm", ["push", archivePath, "oci://ghcr.io/ugoite/charts"]);
  }
  await verifyPublishedHelm(
    chartRef,
    candidate.manifest.version,
    archivePath,
  );
}

async function verifyPublishedHelm(
  chartRef: string,
  version: string,
  archivePath: string,
): Promise<void> {
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-helm-" });
  try {
    await run("helm", [
      "pull",
      chartRef,
      "--version",
      version,
      "--destination",
      tempDir,
    ]);
    const published = pathJoin(tempDir, basename(archivePath));
    if (
      !bytesEqual(
        await Deno.readFile(archivePath),
        await Deno.readFile(published),
      )
    ) {
      throw new Error(`published Helm chart ${version} differs from candidate`);
    }
  } finally {
    await Deno.remove(tempDir, { recursive: true }).catch(() => {});
  }
  console.log(`Helm chart ${version} matches candidate`);
}

async function publishContainer(candidate: VerifiedCandidate): Promise<void> {
  const artifact = candidate.manifest.artifacts.find((entry) =>
    entry.kind === "image"
  );
  const config = artifact?.config ?? {};
  const repository = config.repository;
  const sourceTag = config.tag;
  const expectedDigest = config.digest;
  if (!repository || !sourceTag || !expectedDigest) {
    throw new Error("candidate container coordinates are incomplete");
  }
  const sourceRef = `${repository}:${sourceTag}`;
  const inspect = await run("docker", [
    "buildx",
    "imagetools",
    "inspect",
    sourceRef,
    "--format",
    "{{json .Manifest.Digest}}",
  ]);
  const actualDigest = inspect.stdout.replaceAll('"', "").trim();
  if (actualDigest !== expectedDigest) {
    throw new Error(
      `candidate container digest ${actualDigest} differs from ${expectedDigest}`,
    );
  }
  const releaseRef = `${repository}:${candidate.manifest.version}`;
  const existing = await tryRun("docker", [
    "buildx",
    "imagetools",
    "inspect",
    releaseRef,
    "--format",
    "{{json .Manifest.Digest}}",
  ]);
  if (!existing.success && !isMissing(existing.stderr)) {
    throw new Error(existing.stderr);
  }
  if (existing.success) {
    if (existing.stdout.replaceAll('"', "").trim() !== expectedDigest) {
      throw new Error(
        `published container ${releaseRef} differs from candidate`,
      );
    }
  } else {
    await run("docker", [
      "buildx",
      "imagetools",
      "create",
      "--tag",
      releaseRef,
      `${sourceRef}@${expectedDigest}`,
    ]);
  }
  const published = await run("docker", [
    "buildx",
    "imagetools",
    "inspect",
    releaseRef,
    "--format",
    "{{json .Manifest.Digest}}",
  ]);
  if (published.stdout.replaceAll('"', "").trim() !== expectedDigest) {
    throw new Error(
      `published container ${releaseRef} differs from candidate`,
    );
  }
}

async function packageCli(): Promise<void> {
  const version = (await readVersionState()).versionFile;
  const target = Deno.env.get("UGOITE_CLI_TARGET")?.trim();
  if (!target) throw new Error("UGOITE_CLI_TARGET must be set");
  const binaryPath = Deno.env.get("UGOITE_CLI_BINARY_PATH")?.trim() ??
    "target/rust/release/ugoite";
  await ensureFile(pathJoin(binaryPath), "ugoite CLI binary");
  const archivePath = pathJoin(
    "target",
    "artifacts",
    "cli",
    `ugoite-v${version}-${target}.tar.gz`,
  );
  await Deno.mkdir(dirname(archivePath), { recursive: true });
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-cli-package-" });
  await Deno.copyFile(pathJoin(binaryPath), pathJoin(tempDir, "ugoite"));
  await run("tar", ["-C", tempDir, "-czf", archivePath, "ugoite"]);
  await writeChecksumFile(archivePath, `${archivePath}.sha256`);
}

async function verifyCli(): Promise<void> {
  const version = (await readVersionState()).versionFile;
  const target = Deno.env.get("UGOITE_CLI_TARGET")?.trim();
  if (!target) throw new Error("UGOITE_CLI_TARGET must be set");
  const archivePath = pathJoin(
    "target",
    "artifacts",
    "cli",
    `ugoite-v${version}-${target}.tar.gz`,
  );
  await verifyChecksumFile(archivePath, `${archivePath}.sha256`);
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-cli-verify-" });
  await run("tar", ["-xzf", archivePath, "-C", tempDir]);
  const output = await run(pathJoin(tempDir, "ugoite"), ["--version"]);
  if (!output.stdout.includes(version)) {
    throw new Error(
      `ugoite --version must contain ${version}, got ${output.stdout}`,
    );
  }
}

async function packageNpm(): Promise<void> {
  const targetDir = pathJoin(Deno.cwd(), "target", "artifacts", "npm");
  await Deno.mkdir(targetDir, { recursive: true });
  const result = await run("npm", [
    "pack",
    "--json",
    "--pack-destination",
    targetDir,
  ], pathJoin("packages", "ugoite"));
  const parsed = JSON.parse(result.stdout) as Array<{ filename?: string }>;
  const filename = parsed[0]?.filename;
  if (!filename) {
    throw new Error(
      `npm pack did not report an output filename: ${result.stdout}`,
    );
  }
  await writeChecksumFile(
    pathJoin(targetDir, filename),
    pathJoin(targetDir, `${filename}.sha256`),
  );
}

async function packageHelm(): Promise<void> {
  const targetDir = pathJoin("target", "artifacts", "helm");
  await Deno.mkdir(targetDir, { recursive: true });
  for await (const entry of Deno.readDir(targetDir)) {
    if (
      entry.isFile &&
      (entry.name.endsWith(".tgz") || entry.name.endsWith(".tgz.sha256"))
    ) {
      await Deno.remove(pathJoin(targetDir, entry.name));
    }
  }
  await run("helm", [
    "package",
    "charts/ugoite",
    "--destination",
    targetDir,
  ]);
  const archives: string[] = [];
  for await (const entry of Deno.readDir(targetDir)) {
    if (entry.isFile && entry.name.endsWith(".tgz")) {
      archives.push(pathJoin(targetDir, entry.name));
    }
  }
  if (archives.length !== 1) {
    throw new Error(`expected exactly one Helm archive in ${targetDir}`);
  }
  await writeChecksumFile(archives[0], `${archives[0]}.sha256`);
}

async function verifyNpm(): Promise<void> {
  const version = (await readVersionState()).versionFile;
  const tarballPath = pathJoin(
    "target",
    "artifacts",
    "npm",
    `ugoite-ugoite-${version}.tgz`,
  );
  await verifyChecksumFile(tarballPath, `${tarballPath}.sha256`);
  const inspect = JSON.parse(
    (await run(
      "npm",
      ["pack", "--dry-run", "--json"],
      pathJoin("packages", "ugoite"),
    )).stdout,
  ) as Array<{ name?: string; version?: string }>;
  if (
    inspect[0]?.name !== "@ugoite/ugoite" || inspect[0]?.version !== version
  ) throw new Error("npm package metadata does not match canonical version");
}

async function verifyHelmPackage(): Promise<void> {
  const version = (await readVersionState()).versionFile;
  const archivePath = pathJoin(
    "target",
    "artifacts",
    "helm",
    `ugoite-${version}.tgz`,
  );
  await ensureFile(archivePath, "Helm chart archive");
  await verifyChecksumFile(archivePath, `${archivePath}.sha256`);
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-helm-verify-" });
  await run("tar", ["-xzf", archivePath, "-C", tempDir]);
  await run("helm", ["lint", pathJoin(tempDir, "ugoite")]);
  await run("helm", [
    "template",
    "ugoite",
    pathJoin(tempDir, "ugoite"),
    "--set",
    "nodeSecret.existingSecret=ugoite-node-secret",
  ]);
}

async function readVersionState(): Promise<VersionState> {
  const cargo = await readText("Cargo.toml");
  const packageJson = JSON.parse(
    await readText(pathJoin("packages", "ugoite", "package.json")),
  ) as {
    name?: string;
    version?: string;
    publishConfig?: { registry?: string };
  };
  const chart = await readText(pathJoin("charts", "ugoite", "Chart.yaml"));
  const values = await readText(pathJoin("charts", "ugoite", "values.yaml"));
  return {
    workspace: capture(
      cargo,
      /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
      "workspace version",
    ),
    npmPackage: packageJson.version ?? fail("npm package version is missing"),
    helmChart: capture(chart, /^version:\s*([^\n]+)$/m, "Helm chart version"),
    helmApp: capture(chart, /^appVersion:\s*([^\n]+)$/m, "Helm appVersion"),
    helmImageTag: capture(
      values,
      /^\x20\x20tag:\s*([^\n]+)$/m,
      "Helm image tag",
    ),
    versionFile: (await readText("version.txt")).trim(),
    npmPackageName: packageJson.name ?? fail("npm package name is missing"),
    npmRegistry: packageJson.publishConfig?.registry ??
      fail("npm registry is missing"),
  };
}

async function workspacePackageNames(): Promise<string[]> {
  const names: string[] = [];
  for await (const entry of Deno.readDir(pathJoin("crates"))) {
    if (!entry.isDirectory) continue;
    const manifestPath = pathJoin("crates", entry.name, "Cargo.toml");
    try {
      const manifest = await Deno.readTextFile(manifestPath);
      const match = manifest.match(/^name\s*=\s*"([^"]+)"/m);
      if (match) names.push(match[1]);
    } catch {
      // A non-crate directory is not a workspace package.
    }
  }
  return names;
}

function parseStableVersion(value: string): Version {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(value.trim());
  if (!match) {
    throw new Error(`version must be stable SemVer x.y.z, got ${value}`);
  }
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
  };
}

function compareVersions(a: Version, b: Version): number {
  return a.major - b.major || a.minor - b.minor || a.patch - b.patch;
}

function formatVersion(version: Version): string {
  return `${version.major}.${version.minor}.${version.patch}`;
}

function candidateManifestPath(args: string[]): string {
  return flagValue(args, "--candidate") ??
    Deno.env.get("UGOITE_CANDIDATE_MANIFEST") ??
    pathJoin("target", "artifacts", "candidate-manifest.json");
}

function flagValue(args: string[], flag: string): string | undefined {
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] : undefined;
}

function safeCandidatePath(directory: string, relative: string): string {
  if (
    !relative || relative.startsWith("/") || relative.split("/").includes("..")
  ) throw new Error(`unsafe candidate artifact path ${relative}`);
  return pathJoin(directory, relative);
}

async function readText(relative: string): Promise<string> {
  return await Deno.readTextFile(pathJoin(relative));
}

async function replaceLine(
  relative: string,
  pattern: RegExp,
  replacement: string,
): Promise<void> {
  const filePath = pathJoin(relative);
  const text = await Deno.readTextFile(filePath);
  if (!pattern.test(text)) {
    throw new Error(`${relative} projection was not found`);
  }
  const next = text.replace(pattern, replacement);
  if (next !== text) await Deno.writeTextFile(filePath, next);
}

async function ensureFile(filePath: string, label: string): Promise<void> {
  try {
    const stat = await Deno.stat(filePath);
    if (!stat.isFile) throw new Error(`${label} must be a file: ${filePath}`);
  } catch {
    throw new Error(`${label} was not found at ${filePath}`);
  }
}

async function verifyChecksumFile(
  archivePath: string,
  checksumPath: string,
): Promise<void> {
  await ensureFile(archivePath, "archive");
  await ensureFile(checksumPath, "checksum");
  const [expectedDigest, expectedFile] = (await Deno.readTextFile(checksumPath))
    .trim().split(/\s+/, 2);
  if (expectedFile !== basename(archivePath)) {
    throw new Error(
      `checksum file records ${expectedFile}, expected ${
        basename(archivePath)
      }`,
    );
  }
  if (expectedDigest !== await sha256File(archivePath)) {
    throw new Error(`checksum mismatch for ${archivePath}`);
  }
}

async function writeChecksumFile(
  archivePath: string,
  checksumPath: string,
): Promise<void> {
  await Deno.writeTextFile(
    checksumPath,
    `${await sha256File(archivePath)}  ${basename(archivePath)}\n`,
  );
}

async function sha256File(filePath: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    await Deno.readFile(filePath),
  );
  return [...new Uint8Array(digest)].map((value) =>
    value.toString(16).padStart(2, "0")
  ).join("");
}

async function run(
  cmd: string,
  args: string[],
  cwd = repoRoot,
): Promise<{ stdout: string; stderr: string }> {
  const output = await new Deno.Command(cmd, {
    args,
    cwd,
    stdout: "piped",
    stderr: "piped",
  }).output();
  const stdout = decoder.decode(output.stdout).trim();
  const stderr = decoder.decode(output.stderr).trim();
  if (!output.success) {
    throw new Error(
      `${cmd} ${args.join(" ")} failed\n${stdout}${
        stdout && stderr ? "\n" : ""
      }${stderr}`,
    );
  }
  return { stdout, stderr };
}

async function tryRun(
  cmd: string,
  args: string[],
  cwd = repoRoot,
): Promise<{ success: boolean; stdout: string; stderr: string }> {
  try {
    const result = await run(cmd, args, cwd);
    return { success: true, ...result };
  } catch (error) {
    return {
      success: false,
      stdout: "",
      stderr: error instanceof Error ? error.message : String(error),
    };
  }
}

function isMissing(message: string): boolean {
  return /404|e404|not found|manifest unknown|name unknown/i.test(message);
}

function pathJoin(...parts: string[]): string {
  const first = parts[0]?.startsWith("/") ? "/" : "";
  return first +
    parts.join("/").replace(/^\/+/, "").replace(/\/+/g, "/").replace(
      /\/\.\//g,
      "/",
    );
}

function dirname(filePath: string): string {
  const index = filePath.lastIndexOf("/");
  return index <= 0 ? (index === 0 ? "/" : ".") : filePath.slice(0, index);
}

function basename(filePath: string): string {
  return filePath.split("/").at(-1) ?? filePath;
}

function bytesEqual(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length &&
    left.every((value, index) => value === right[index]);
}

function capture(text: string, pattern: RegExp, label: string): string {
  const match = text.match(pattern);
  if (!match) fail(`${label} not found`);
  return match[1].trim().replace(/\s+#.*$/, "").replace(/^"|"$/g, "");
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function fail(message: string): never {
  throw new Error(message);
}
