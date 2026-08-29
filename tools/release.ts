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

type CandidateFile = {
  path: string;
  sha256: string;
  size: number;
};

type CandidateArtifact = {
  kind: "cli" | "npm" | "helm" | "image" | "release";
  files: CandidateFile[];
  config?: Record<string, string>;
};

type CandidateManifest = {
  schema_version: number;
  contract_version: number;
  version: string;
  source_sha: string;
  ci_run_id: string;
  verification: { release_grade: string };
  artifacts: CandidateArtifact[];
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
      await verifyCandidate(candidateManifestPath(args));
      break;
    case "candidate-id":
      console.log(`sha256:${await sha256File(candidateManifestPath(args))}`);
      break;
    case "promote":
      await promote(await verifyCandidate(candidateManifestPath(args), args));
      break;
    case "promote-aliases":
      await promoteAliases(
        await verifyCandidate(candidateManifestPath(args), args),
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
        "usage: deno run -A tools/release.ts <version-sync|version-check|prepare compatible|prepare breaking|candidate|verify-candidate|candidate-id|promote|promote-aliases|package-cli|verify-cli|package-npm|package-helm|verify-npm|verify-helm>",
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

async function createCandidate(): Promise<void> {
  if (Deno.env.get("UGOITE_RELEASE_CANDIDATE_PREBUILT") !== "true") {
    await run("mise", ["run", "ci:release"]);
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
): Promise<VerifiedCandidate> {
  await ensureFile(manifestPath, "candidate manifest");
  const bytes = await Deno.readFile(manifestPath);
  const manifest = JSON.parse(
    new TextDecoder().decode(bytes),
  ) as CandidateManifest;
  if (manifest.schema_version !== 2) {
    throw new Error("candidate manifest schema_version must be 2");
  }
  if (manifest.contract_version !== 2) {
    throw new Error("candidate manifest contract_version must be 2");
  }
  if (!Array.isArray(manifest.artifacts)) {
    throw new Error("candidate manifest artifacts must be an array");
  }
  parseStableVersion(manifest.version);
  if (!/^[0-9a-f]{40}$/.test(manifest.source_sha)) {
    throw new Error(
      "candidate source_sha must be a 40-character Git commit SHA",
    );
  }
  if (!manifest.ci_run_id) throw new Error("candidate ci_run_id is required");
  if (manifest.verification?.release_grade !== "passed") {
    throw new Error("candidate verification.release_grade must be passed");
  }
  const candidateId = `sha256:${await sha256File(manifestPath)}`;
  const expectedId = flagValue(args, "--candidate-id") ??
    Deno.env.get("UGOITE_CANDIDATE_ID");
  if (expectedId && expectedId !== candidateId) {
    throw new Error(
      `candidate id ${candidateId} does not match requested ${expectedId}`,
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

async function promote(candidate: VerifiedCandidate): Promise<void> {
  if (Deno.env.get("UGOITE_PROMOTION_DRY_RUN") === "true") {
    console.log(
      `dry-run promotion ${candidate.candidateId} for v${candidate.manifest.version}`,
    );
    return;
  }
  const version = candidate.manifest.version;
  const stableTag = `v${version}`;
  const draftTag = candidateDraftTag(candidate);
  await ensureDraftRelease(draftTag, candidate.manifest.source_sha);
  await publishCliAssets(candidate, draftTag);
  await publishNpm(candidate);
  await publishHelm(candidate);
  await publishContainer(candidate);
  await publishReleaseLedgerAssets(candidate, draftTag);
  await ensureStableRelease(stableTag, candidate.manifest.source_sha);
  await publishCliAssets(candidate, stableTag);
  await publishReleaseLedgerAssets(candidate, stableTag);
  await run("gh", ["release", "edit", stableTag, "--draft=false"]);
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
  console.log(`updated mutable aliases from ${sourceTag}`);
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
): Promise<void> {
  const existing = await tryRun("gh", [
    "release",
    "view",
    tag,
    "--json",
    "tagName,targetCommitish,isDraft",
  ]);
  if (existing.success) {
    const release = JSON.parse(existing.stdout) as {
      tagName?: string;
      targetCommitish?: string;
      isDraft?: boolean;
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
    "--title",
    `Ugoite ${tag}`,
    "--target",
    sourceSha,
    "--notes",
    "Verified Ugoite release candidate.",
  ]);
}

async function publishCliAssets(
  candidate: VerifiedCandidate,
  tag: string,
): Promise<void> {
  const assets = candidate.manifest.artifacts
    .filter((artifact) => artifact.kind === "cli")
    .flatMap((artifact) => artifact.files)
    .map((file) =>
      safeCandidatePath(dirname(candidate.manifestPath), file.path)
    );
  await publishReleaseFiles(tag, assets);
}

async function publishReleaseLedgerAssets(
  candidate: VerifiedCandidate,
  tag: string,
): Promise<void> {
  const directory = dirname(candidate.manifestPath);
  const idPath = pathJoin(directory, "candidate-id.txt");
  const publicManifestPath = pathJoin(directory, "release-manifest.json");
  const image = candidate.manifest.artifacts.find((artifact) =>
    artifact.kind === "image"
  );
  const helm = candidate.manifest.artifacts.find((artifact) =>
    artifact.kind === "helm"
  );
  const releaseAssets = candidate.manifest.artifacts
    .filter((artifact) => artifact.kind === "release")
    .flatMap((artifact) => artifact.files)
    .map((file) => safeCandidatePath(directory, file.path));
  const publicManifest = {
    schema_version: 2,
    release_tag: `v${candidate.manifest.version}`,
    version: candidate.manifest.version,
    source_sha: candidate.manifest.source_sha,
    candidate_id: candidate.candidateId,
    files: candidate.manifest.artifacts.flatMap((artifact) => artifact.files)
      .map((file) => ({
        name: basename(file.path),
        sha256: file.sha256,
        size: file.size,
      })),
    image: {
      repository: image?.config?.repository ?? "",
      digest: image?.config?.digest ?? "",
    },
    helm_chart: {
      repository: "oci://ghcr.io/ugoite/charts/ugoite",
      digest: helm?.files.find((file) => file.path.endsWith(".tgz"))?.sha256 ??
        "",
    },
  };
  await Deno.writeTextFile(
    publicManifestPath,
    `${JSON.stringify(publicManifest, null, 2)}\n`,
  );
  await Deno.writeTextFile(idPath, `${candidate.candidateId}\n`);
  await publishReleaseFiles(tag, [
    candidate.manifestPath,
    publicManifestPath,
    idPath,
    ...releaseAssets,
  ]);
}

function candidateDraftTag(candidate: VerifiedCandidate): string {
  const shortSource = candidate.manifest.source_sha.slice(0, 12);
  const shortCandidate = candidate.candidateId.slice(-12);
  return `candidate-${shortSource}-${shortCandidate}`;
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
    await run("npm", ["publish", tarballPath, "--tag", "latest"]);
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
  const targetDir = pathJoin("target", "artifacts", "npm");
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
