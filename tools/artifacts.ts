const decoder = new TextDecoder();
const repoRoot = decodeURIComponent(new URL("../", import.meta.url).pathname)
  .replace(/\/$/, "");
const artifactRoot = Deno.env.get("UGOITE_ARTIFACT_ROOT")?.trim()
  ? pathJoin(repoRoot, Deno.env.get("UGOITE_ARTIFACT_ROOT")!.trim())
  : pathJoin(repoRoot, "target", "artifacts");

type ArtifactKind = "docsite" | "cli" | "helm" | "image" | "npm" | "release";

type ManifestFile = {
  path: string;
  sha256: string;
  size: number;
};

type ManifestArtifact = {
  kind: ArtifactKind;
  logical_name: string;
  build_profile: string;
  path: string;
  files: ManifestFile[];
  config: Record<string, string>;
};

type ArtifactManifest = {
  schema_version: 2;
  version: string;
  source_sha: string | null;
  ci_run_id: string | null;
  contract_version: 2;
  generated_at: string;
  verification: { release_grade: string };
  artifacts: ManifestArtifact[];
};

if (import.meta.main) await main();

async function main(): Promise<void> {
  switch (Deno.args[0]) {
    case "package-docsite":
      await packageDocsite();
      break;
    case "write-manifest":
      await writeManifest();
      break;
    case "write-candidate-manifest":
      await writeCandidateManifest();
      break;
    case "verify-docsite":
      await verifyDocsite();
      break;
    case "verify-cli":
      await verifyCli();
      break;
    case "verify-helm":
      await verifyHelm();
      break;
    case "verify-image":
      await verifyImage();
      break;
    default:
      throw new Error(
        "usage: deno run -A tools/artifacts.ts <package-docsite|write-manifest|write-candidate-manifest|verify-docsite|verify-cli|verify-helm|verify-image>",
      );
  }
}

async function packageDocsite(): Promise<void> {
  const sourceDir = pathJoin(repoRoot, "docsite", "dist");
  const destinationDir = pathJoin(artifactRoot, "docsite", "site");
  await ensureDirectory(sourceDir, "docsite build output");
  await Deno.remove(destinationDir, { recursive: true }).catch(() => {});
  await copyDir(sourceDir, destinationDir);
}

async function writeManifest(): Promise<void> {
  const manifest = await buildManifest(false);
  const required = new Set(manifest.artifacts.map((artifact) => artifact.kind));
  for (const kind of ["docsite", "cli", "helm", "image"] as const) {
    if (!required.has(kind)) {
      throw new Error(`artifact manifest is missing ${kind}`);
    }
  }
  await writeJson(pathJoin(artifactRoot, "manifest.json"), manifest);
  await writeChecksums(manifest, pathJoin(artifactRoot, "SHA256SUMS"));
}

async function writeCandidateManifest(): Promise<void> {
  const manifest = await buildManifest(true);
  const required = new Set(manifest.artifacts.map((artifact) => artifact.kind));
  for (const kind of ["cli", "npm", "helm", "image", "release"] as const) {
    if (!required.has(kind)) {
      throw new Error(`candidate artifact set is missing ${kind}`);
    }
  }
  if (manifest.verification.release_grade !== "passed") {
    throw new Error(
      "UGOITE_RELEASE_GRADE=passed is required for a candidate manifest",
    );
  }
  await writeJson(pathJoin(artifactRoot, "candidate-manifest.json"), manifest);
  await writeChecksums(manifest, pathJoin(artifactRoot, "SHA256SUMS"));
}

async function buildManifest(candidate: boolean): Promise<ArtifactManifest> {
  const version = (await Deno.readTextFile(pathJoin(repoRoot, "version.txt")))
    .trim();
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
    throw new Error(`version.txt must contain stable SemVer, got ${version}`);
  }
  const sourceSha = await sourceIdentity(candidate);
  const ciRunId = Deno.env.get("UGOITE_CI_RUN_ID")?.trim() ??
    Deno.env.get("GITHUB_RUN_ID")?.trim() ?? (candidate ? null : null);
  if (candidate && (!sourceSha || !ciRunId)) {
    throw new Error(
      "candidate manifest requires source SHA and CI run identity",
    );
  }

  const artifacts: ManifestArtifact[] = [];
  if (candidate) {
    const releaseFiles = [
      "docker-compose.release.yaml",
      "docker-compose.release.yaml.sha256",
    ]
      .map((name) => pathJoin(artifactRoot, name));
    if (
      (await Promise.all(releaseFiles.map((path) => isFile(path)))).every(
        Boolean,
      )
    ) {
      artifacts.push({
        kind: "release",
        logical_name: "ugoite-release-assets",
        build_profile: "release",
        path: ".",
        files: await Promise.all(releaseFiles.map(async (path) => ({
          path: relativeFromArtifactRoot(path),
          sha256: await sha256File(path),
          size: (await Deno.stat(path)).size,
        }))),
        config: { version },
      });
    }
  }
  const docsiteDir = pathJoin(artifactRoot, "docsite", "site");
  if (await isDirectory(docsiteDir)) {
    artifacts.push({
      kind: "docsite",
      logical_name: "ugoite-docsite-pages",
      build_profile: "release",
      path: "docsite/site",
      files: await collectFiles(docsiteDir),
      config: {
        origin: Deno.env.get("DOCSITE_ORIGIN") ?? "",
        base: Deno.env.get("DOCSITE_BASE") ?? "/",
        version,
      },
    });
  }

  const cliDir = pathJoin(artifactRoot, "cli");
  if (await isDirectory(cliDir)) {
    if (candidate) {
      artifacts.push(...await candidateCliArtifacts(cliDir));
    } else {
      await ensureSingleFile(cliDir, /\.tar\.gz$/, "CLI package");
      artifacts.push({
        kind: "cli",
        logical_name: "ugoite-cli-linux",
        build_profile: "release",
        path: "cli",
        files: await collectFiles(cliDir),
        config: {
          platform: Deno.env.get("UGOITE_CLI_TARGET") ??
            "x86_64-unknown-linux-gnu",
          version,
        },
      });
    }
  }

  const helmDir = pathJoin(artifactRoot, "helm");
  if (await isDirectory(helmDir)) {
    await ensureSingleFile(helmDir, /\.tgz$/, "Helm package");
    artifacts.push({
      kind: "helm",
      logical_name: "ugoite-helm-chart",
      build_profile: "release",
      path: "helm",
      files: await collectFiles(helmDir),
      config: { chart: "ugoite", version },
    });
  }

  const npmDir = pathJoin(artifactRoot, "npm");
  if (await isDirectory(npmDir)) {
    await ensureSingleFile(npmDir, /\.tgz$/, "npm package");
    artifacts.push({
      kind: "npm",
      logical_name: "ugoite-npm-installer",
      build_profile: "release",
      path: "npm",
      files: await collectFiles(npmDir),
      config: { package: "@ugoite/ugoite", version },
    });
  }

  const imageDir = pathJoin(artifactRoot, "image");
  const imageFiles = await isDirectory(imageDir)
    ? await collectFiles(imageDir)
    : [];
  const imageConfig = await readImageConfig(candidate);
  if (candidate || imageFiles.length > 0) {
    artifacts.push({
      kind: "image",
      logical_name: "ugoite-runtime-image",
      build_profile: "release",
      path: "image",
      files: imageFiles,
      config: imageConfig,
    });
  }

  return {
    schema_version: 2,
    version,
    source_sha: sourceSha,
    ci_run_id: ciRunId,
    contract_version: 2,
    generated_at: new Date().toISOString(),
    verification: {
      release_grade: Deno.env.get("UGOITE_RELEASE_GRADE") ?? "not_run",
    },
    artifacts,
  };
}

async function candidateCliArtifacts(
  cliDir: string,
): Promise<ManifestArtifact[]> {
  const directories: Array<{ directory: string; platform: string }> = [];
  for await (const entry of Deno.readDir(cliDir)) {
    if (entry.isDirectory) {
      directories.push({
        directory: pathJoin(cliDir, entry.name),
        platform: entry.name,
      });
    }
  }
  if (directories.length === 0) {
    const files = await collectFiles(cliDir);
    const archive = files.find((file) => file.path.endsWith(".tar.gz"));
    if (!archive) {
      throw new Error("candidate CLI package is missing an archive");
    }
    directories.push({
      directory: cliDir,
      platform: archive.path.replace(/^.*ugoite-v[^-]+-/, "").replace(
        /\.tar\.gz$/,
        "",
      ),
    });
  }
  return await Promise.all(directories.map(async ({ directory, platform }) => {
    const files = await collectFiles(directory);
    if (!files.some((file) => file.path.endsWith(".tar.gz"))) {
      throw new Error(
        `candidate CLI package ${platform} is missing an archive`,
      );
    }
    return {
      kind: "cli" as const,
      logical_name: `ugoite-cli-${platform}`,
      build_profile: "release",
      path: relativeFromArtifactRoot(directory),
      files,
      config: { platform },
    };
  }));
}

async function verifyDocsite(): Promise<void> {
  const manifest = await readManifest("manifest.json");
  const artifact = requireArtifact(manifest, "docsite");
  const html = await Deno.readTextFile(
    pathJoin(artifactRoot, artifact.path, "index.html"),
  );
  if (html.trim().length === 0) throw new Error("docsite index.html is empty");
  if (
    artifact.config.origin && !artifact.config.origin.includes("localhost") &&
    html.includes("http://localhost")
  ) {
    throw new Error("production docsite artifact still references localhost");
  }
  await verifyManifestFiles(artifact);
}

async function verifyCli(): Promise<void> {
  const manifest = await readManifest("manifest.json");
  const artifact = requireArtifact(manifest, "cli");
  await verifyManifestFiles(artifact);
  const archive = artifact.files.find((file) => file.path.endsWith(".tar.gz"));
  if (!archive) throw new Error("CLI archive is missing from manifest");
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-cli-verify-" });
  await run("tar", [
    "-xzf",
    pathJoin(artifactRoot, archive.path),
    "-C",
    tempDir,
  ]);
  await run(pathJoin(tempDir, "ugoite"), ["--version"]);
}

async function verifyHelm(): Promise<void> {
  const manifest = await readManifest("manifest.json");
  const artifact = requireArtifact(manifest, "helm");
  await verifyManifestFiles(artifact);
  const archive = artifact.files.find((file) => file.path.endsWith(".tgz"));
  if (!archive) throw new Error("Helm package is missing from manifest");
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-helm-" });
  await run("tar", [
    "-xzf",
    pathJoin(artifactRoot, archive.path),
    "-C",
    tempDir,
  ]);
  await run("helm", ["lint", pathJoin(tempDir, "ugoite")]);
}

async function verifyImage(): Promise<void> {
  const manifest = await readManifest("manifest.json");
  const artifact = requireArtifact(manifest, "image");
  await verifyManifestFiles(artifact);
  const archive = artifact.files.find((file) => file.path.endsWith(".tar.gz"));
  if (!archive) throw new Error("image archive is missing from manifest");
  const archivePath = pathJoin(artifactRoot, archive.path);
  await run("gzip", ["-t", archivePath]);
  const imageManifest = JSON.parse(
    await readTarGzEntry(archivePath, "manifest.json"),
  ) as Array<{ Config?: string }>;
  const configPath = imageManifest[0]?.Config;
  if (!configPath) throw new Error("docker archive manifest is missing Config");
  const config = JSON.parse(await readTarGzEntry(archivePath, configPath)) as {
    config?: { Entrypoint?: string[]; User?: string };
  };
  if (
    JSON.stringify(config.config?.Entrypoint ?? []) !==
      JSON.stringify(["ugoite-server"])
  ) throw new Error("unexpected image entrypoint");
  if ((config.config?.User ?? "") !== "ugoite") {
    throw new Error("unexpected image user");
  }
}

async function verifyManifestFiles(artifact: ManifestArtifact): Promise<void> {
  for (const file of artifact.files) {
    const filePath = pathJoin(artifactRoot, file.path);
    const actual = await sha256File(filePath);
    if (actual !== file.sha256) {
      throw new Error(`checksum mismatch for ${file.path}`);
    }
    if ((await Deno.stat(filePath)).size !== file.size) {
      throw new Error(`size mismatch for ${file.path}`);
    }
  }
}

async function readManifest(name: string): Promise<ArtifactManifest> {
  return JSON.parse(
    await Deno.readTextFile(pathJoin(artifactRoot, name)),
  ) as ArtifactManifest;
}

function requireArtifact(
  manifest: ArtifactManifest,
  kind: ArtifactKind,
): ManifestArtifact {
  const artifact = manifest.artifacts.find((entry) => entry.kind === kind);
  if (!artifact) throw new Error(`manifest is missing ${kind} artifact`);
  return artifact;
}

async function collectFiles(baseDir: string): Promise<ManifestFile[]> {
  const files: ManifestFile[] = [];
  for await (const filePath of walkFiles(baseDir)) {
    files.push({
      path: relativeFromArtifactRoot(filePath),
      sha256: await sha256File(filePath),
      size: (await Deno.stat(filePath)).size,
    });
  }
  return files.sort((left, right) => left.path.localeCompare(right.path));
}

async function* walkFiles(baseDir: string): AsyncGenerator<string> {
  for await (const entry of Deno.readDir(baseDir)) {
    const child = pathJoin(baseDir, entry.name);
    if (entry.isDirectory) yield* walkFiles(child);
    else if (entry.isFile) yield child;
  }
}

async function copyDir(
  sourceDir: string,
  destinationDir: string,
): Promise<void> {
  await Deno.mkdir(destinationDir, { recursive: true });
  for await (const entry of Deno.readDir(sourceDir)) {
    const source = pathJoin(sourceDir, entry.name);
    const destination = pathJoin(destinationDir, entry.name);
    if (entry.isDirectory) await copyDir(source, destination);
    else if (entry.isSymlink) {
      throw new Error(`unexpected symlink in artifact source: ${source}`);
    } else if (entry.isFile) await Deno.copyFile(source, destination);
  }
}

async function writeChecksums(
  manifest: ArtifactManifest,
  path: string,
): Promise<void> {
  const lines = manifest.artifacts.flatMap((artifact) =>
    artifact.files.map((file) => `${file.sha256}  ${file.path}`)
  ).sort();
  await Deno.writeTextFile(
    path,
    `${lines.join("\n")}${lines.length ? "\n" : ""}`,
  );
}

async function writeJson(path: string, value: unknown): Promise<void> {
  await Deno.mkdir(dirname(path), { recursive: true });
  await Deno.writeTextFile(path, `${JSON.stringify(value, null, 2)}\n`);
}

async function sourceIdentity(candidate: boolean): Promise<string | null> {
  const explicit = Deno.env.get("UGOITE_SOURCE_SHA")?.trim() ??
    Deno.env.get("GITHUB_SHA")?.trim();
  if (explicit) return explicit;
  if (!candidate) return null;
  const result = await run("git", ["rev-parse", "HEAD"]);
  return result.stdout;
}

async function readImageConfig(
  candidate: boolean,
): Promise<Record<string, string>> {
  const config: Record<string, string> = {
    repository: Deno.env.get("UGOITE_CONTAINER_REPOSITORY") ??
      "ghcr.io/ugoite/ugoite",
    tag: Deno.env.get("UGOITE_CONTAINER_TAG") ?? "",
    platform: Deno.env.get("UGOITE_IMAGE_PLATFORM") ??
      "linux/amd64,linux/arm64",
  };
  const digest = Deno.env.get("UGOITE_CONTAINER_DIGEST")?.trim();
  if (digest) config.digest = digest;
  if (candidate && !config.tag) {
    throw new Error("UGOITE_CONTAINER_TAG is required for candidate manifests");
  }
  if (candidate && !config.digest) {
    throw new Error(
      "UGOITE_CONTAINER_DIGEST is required for candidate manifests",
    );
  }
  return config;
}

async function ensureDirectory(path: string, label: string): Promise<void> {
  if (!(await isDirectory(path))) {
    throw new Error(`${label} was not found at ${path}`);
  }
}

async function isDirectory(path: string): Promise<boolean> {
  try {
    return (await Deno.stat(path)).isDirectory;
  } catch {
    return false;
  }
}

async function isFile(path: string): Promise<boolean> {
  try {
    return (await Deno.stat(path)).isFile;
  } catch {
    return false;
  }
}

async function ensureSingleFile(
  dir: string,
  pattern: RegExp,
  label: string,
): Promise<void> {
  const matches: string[] = [];
  for await (const entry of Deno.readDir(dir)) {
    if (entry.isFile && pattern.test(entry.name)) matches.push(entry.name);
  }
  if (matches.length !== 1) {
    throw new Error(`${label} expected exactly one matching file in ${dir}`);
  }
}

async function sha256File(path: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    await Deno.readFile(path),
  );
  return [...new Uint8Array(digest)].map((value) =>
    value.toString(16).padStart(2, "0")
  ).join("");
}

async function readTarGzEntry(
  archivePath: string,
  entryPath: string,
): Promise<string> {
  return (await run("tar", ["-xOzf", archivePath, entryPath])).stdout;
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

function relativeFromArtifactRoot(path: string): string {
  const prefix = `${artifactRoot}/`;
  if (!path.startsWith(prefix)) {
    throw new Error(`artifact path escapes root: ${path}`);
  }
  return path.slice(prefix.length);
}

function pathJoin(...parts: string[]): string {
  const configured = parts.find((part, index) =>
    index > 0 && part.startsWith("/")
  );
  if (configured) {
    return pathJoin(configured, ...parts.slice(parts.indexOf(configured) + 1));
  }
  const absolute = parts[0]?.startsWith("/") ?? false;
  const joined = parts.join("/").replace(/\/+/g, "/");
  const normalized = joined.replace(/\/\.\//g, "/").replace(/\/\/{2,}/g, "/");
  return absolute
    ? `/${normalized.replace(/^\/+/, "")}`
    : normalized.replace(/^\.\//, "");
}

function dirname(path: string): string {
  const index = path.lastIndexOf("/");
  return index <= 0 ? (index === 0 ? "/" : ".") : path.slice(0, index);
}
