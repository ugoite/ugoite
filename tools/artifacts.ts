const decoder = new TextDecoder();

const repoRoot = new URL("../", import.meta.url);
const artifactRoot = new URL("../target/artifacts/", import.meta.url);
const docsitePackageDir = new URL("docsite/site/", artifactRoot);
const cliPackageDir = new URL("cli/", artifactRoot);
const helmPackageDir = new URL("helm/", artifactRoot);
const imagePackageDir = new URL("image/", artifactRoot);

const DOCSITE_ARTIFACT_NAME = "ugoite-docsite-pages";
const CLI_ARTIFACT_NAME = "ugoite-cli-linux";
const HELM_ARTIFACT_NAME = "ugoite-helm-chart";
const IMAGE_ARTIFACT_NAME = "ugoite-runtime-image";

type ManifestFile = {
  path: string;
  sha256: string;
  size: number;
};

type ManifestArtifact = {
  kind: "docsite" | "cli" | "helm" | "image";
  logical_name: string;
  build_profile: string;
  path: string;
  files: ManifestFile[];
  config: Record<string, string>;
};

type ArtifactManifest = {
  schema_version: 1;
  source_sha: string | null;
  contract_version: 1;
  generated_at: string;
  artifacts: ManifestArtifact[];
};

const command = Deno.args[0];

switch (command) {
  case "package-docsite":
    await packageDocsite();
    break;
  case "write-manifest":
    await writeManifest();
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
      "usage: deno run -A tools/artifacts.ts <package-docsite|write-manifest|verify-docsite|verify-cli|verify-helm|verify-image>",
    );
}

async function packageDocsite(): Promise<void> {
  const sourceDir = new URL("../docsite/dist/", import.meta.url);
  await ensureExists(sourceDir, "docsite build output");
  await Deno.remove(docsitePackageDir, { recursive: true }).catch(() => {});
  await copyDir(sourceDir, docsitePackageDir);
}

async function writeManifest(): Promise<void> {
  await ensureExists(docsitePackageDir, "packaged docsite artifact");
  await ensureSingleFile(cliPackageDir, /\.tar\.gz$/, "CLI package");
  await ensureSingleFile(helmPackageDir, /\.tgz$/, "Helm package");
  await ensureSingleFile(imagePackageDir, /\.tar\.gz$/, "image package");

  const manifest: ArtifactManifest = {
    schema_version: 1,
    source_sha: Deno.env.get("UGOITE_SOURCE_SHA") ??
      Deno.env.get("GITHUB_SHA") ??
      null,
    contract_version: 1,
    generated_at: new Date().toISOString(),
    artifacts: [
      {
        kind: "docsite",
        logical_name: DOCSITE_ARTIFACT_NAME,
        build_profile: "release",
        path: "docsite/site",
        files: await collectFiles(docsitePackageDir),
        config: {
          origin: Deno.env.get("DOCSITE_ORIGIN") ?? "",
          base: Deno.env.get("DOCSITE_BASE") ?? "/",
        },
      },
      {
        kind: "cli",
        logical_name: CLI_ARTIFACT_NAME,
        build_profile: "release",
        path: "cli",
        files: await collectFiles(cliPackageDir),
        config: {
          target: Deno.env.get("UGOITE_CLI_TARGET") ?? "linux-amd64",
          version: await workspaceVersion(),
        },
      },
      {
        kind: "helm",
        logical_name: HELM_ARTIFACT_NAME,
        build_profile: "release",
        path: "helm",
        files: await collectFiles(helmPackageDir),
        config: {
          chart: "ugoite",
          version: await chartVersion(),
        },
      },
      {
        kind: "image",
        logical_name: IMAGE_ARTIFACT_NAME,
        build_profile: "release",
        path: "image",
        files: await collectFiles(imagePackageDir),
        config: {
          tag: Deno.env.get("UGOITE_IMAGE_TAG") ?? "ugoite:e2e",
        },
      },
    ],
  };

  await Deno.mkdir(artifactRoot, { recursive: true });
  await Deno.writeTextFile(
    new URL("manifest.json", artifactRoot),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );

  const sumLines: string[] = [];
  for (const artifact of manifest.artifacts) {
    for (const file of artifact.files) {
      sumLines.push(`${file.sha256}  ${file.path}`);
    }
  }
  sumLines.sort();
  await Deno.writeTextFile(
    new URL("SHA256SUMS", artifactRoot),
    `${sumLines.join("\n")}\n`,
  );
}

async function verifyDocsite(): Promise<void> {
  const manifest = await readManifest();
  const artifact = requireArtifact(manifest, "docsite");
  const indexPath = new URL(`${artifact.path}/index.html`, artifactRoot);
  const html = await Deno.readTextFile(indexPath);
  if (html.trim().length === 0) {
    throw new Error("docsite index.html is empty");
  }
  const origin = artifact.config.origin;
  if (
    origin && !origin.includes("localhost") && html.includes("http://localhost")
  ) {
    throw new Error("production docsite artifact still references localhost");
  }
  await verifyManifestFiles(artifact);
}

async function verifyCli(): Promise<void> {
  const manifest = await readManifest();
  const artifact = requireArtifact(manifest, "cli");
  await verifyManifestFiles(artifact);
  const archive = artifact.files.find((file) => file.path.endsWith(".tar.gz"));
  if (!archive) throw new Error("CLI archive is missing from manifest");
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-cli-" });
  await run("tar", [
    "-xzf",
    pathFromArtifactRoot(archive.path),
    "-C",
    tempDir,
  ]);
  const cliPath = `${tempDir}/ugoite`;
  await Deno.stat(cliPath);
  await run(cliPath, ["--version"]);
}

async function verifyHelm(): Promise<void> {
  const manifest = await readManifest();
  const artifact = requireArtifact(manifest, "helm");
  await verifyManifestFiles(artifact);
  const archive = artifact.files.find((file) => file.path.endsWith(".tgz"));
  if (!archive) throw new Error("Helm package is missing from manifest");
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-helm-" });
  await run("tar", [
    "-xzf",
    pathFromArtifactRoot(archive.path),
    "-C",
    tempDir,
  ]);
  await run("helm", ["lint", `${tempDir}/ugoite`]);
}

async function verifyImage(): Promise<void> {
  const manifest = await readManifest();
  const artifact = requireArtifact(manifest, "image");
  await verifyManifestFiles(artifact);
  const archive = artifact.files.find((file) => file.path.endsWith(".tar.gz"));
  if (!archive) throw new Error("image archive is missing from manifest");
  const archivePath = pathFromArtifactRoot(archive.path);
  await run("gzip", ["-t", archivePath]);

  const imageManifestText = await readTarGzEntry(archivePath, "manifest.json");
  const imageManifest = JSON.parse(imageManifestText) as Array<
    { Config?: string }
  >;
  const configPath = imageManifest[0]?.Config;
  if (!configPath) {
    throw new Error("docker archive manifest is missing Config");
  }

  const configText = await readTarGzEntry(archivePath, configPath);
  const config = JSON.parse(configText) as {
    config?: { Entrypoint?: string[]; User?: string };
  };
  const entrypoint = config.config?.Entrypoint ?? [];
  const user = config.config?.User ?? "";
  if (JSON.stringify(entrypoint) !== JSON.stringify(["ugoite-server"])) {
    throw new Error(
      `unexpected image entrypoint: ${JSON.stringify(entrypoint)}`,
    );
  }
  if (user !== "ugoite") {
    throw new Error(`unexpected image user: ${JSON.stringify(user)}`);
  }
}

async function verifyManifestFiles(artifact: ManifestArtifact): Promise<void> {
  for (const file of artifact.files) {
    const actual = await sha256File(new URL(file.path, artifactRoot));
    if (actual !== file.sha256) {
      throw new Error(`checksum mismatch for ${file.path}`);
    }
  }
}

async function collectFiles(baseDir: URL): Promise<ManifestFile[]> {
  const files: ManifestFile[] = [];
  for await (const fileUrl of walkFiles(baseDir)) {
    const relative = relativeFromArtifactRoot(fileUrl);
    files.push({
      path: relative,
      sha256: await sha256File(fileUrl),
      size: (await Deno.stat(fileUrl)).size,
    });
  }
  files.sort((a, b) => a.path.localeCompare(b.path));
  return files;
}

async function* walkFiles(baseDir: URL): AsyncGenerator<URL> {
  for await (const entry of Deno.readDir(baseDir)) {
    const childUrl = entry.isDirectory
      ? new URL(`${entry.name}/`, baseDir)
      : new URL(entry.name, baseDir);
    if (entry.isDirectory) {
      yield* walkFiles(childUrl);
      continue;
    }
    if (entry.isFile) yield childUrl;
  }
}

async function copyDir(sourceDir: URL, destinationDir: URL): Promise<void> {
  await Deno.mkdir(destinationDir, { recursive: true });
  for await (const entry of Deno.readDir(sourceDir)) {
    const source = new URL(entry.name, sourceDir);
    const destination = new URL(entry.name, destinationDir);
    if (entry.isDirectory) {
      await copyDir(
        new URL(`${entry.name}/`, sourceDir),
        new URL(`${entry.name}/`, destinationDir),
      );
      continue;
    }
    if (entry.isSymlink) {
      throw new Error(
        `unexpected symlink in docsite artifact source: ${source.pathname}`,
      );
    }
    await Deno.copyFile(source, destination);
  }
}

async function sha256File(file: URL): Promise<string> {
  const bytes = await Deno.readFile(file);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((value) =>
    value.toString(16).padStart(2, "0")
  ).join("");
}

function relativeFromArtifactRoot(file: URL): string {
  const artifactPath = pathFromFileUrl(artifactRoot);
  const filePath = pathFromFileUrl(file);
  return filePath.slice(artifactPath.length);
}

function pathFromArtifactRoot(relativePath: string): string {
  return pathFromFileUrl(new URL(relativePath, artifactRoot));
}

function pathFromFileUrl(url: URL): string {
  return decodeURIComponent(url.pathname);
}

async function ensureExists(url: URL, label: string): Promise<void> {
  try {
    await Deno.stat(url);
  } catch {
    throw new Error(`${label} was not found at ${pathFromFileUrl(url)}`);
  }
}

async function ensureSingleFile(
  dir: URL,
  pattern: RegExp,
  label: string,
): Promise<void> {
  await ensureExists(dir, label);
  const matches: string[] = [];
  for await (const entry of Deno.readDir(dir)) {
    if (entry.isFile && pattern.test(entry.name)) matches.push(entry.name);
  }
  if (matches.length !== 1) {
    throw new Error(
      `${label} expected exactly one matching file in ${pathFromFileUrl(dir)}`,
    );
  }
}

async function readManifest(): Promise<ArtifactManifest> {
  const manifestText = await Deno.readTextFile(
    new URL("manifest.json", artifactRoot),
  );
  return JSON.parse(manifestText) as ArtifactManifest;
}

function requireArtifact(
  manifest: ArtifactManifest,
  kind: ManifestArtifact["kind"],
): ManifestArtifact {
  const artifact = manifest.artifacts.find((entry) => entry.kind === kind);
  if (!artifact) throw new Error(`manifest is missing ${kind} artifact`);
  return artifact;
}

async function workspaceVersion(): Promise<string> {
  const cargoToml = await Deno.readTextFile(
    new URL("../Cargo.toml", import.meta.url),
  );
  const workspacePackageBlock = cargoToml.match(
    /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
  );
  const match = workspacePackageBlock ??
    cargoToml.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error("workspace version was not found in Cargo.toml");
  return match[1];
}

async function chartVersion(): Promise<string> {
  const chartYaml = await Deno.readTextFile(
    new URL("../charts/ugoite/Chart.yaml", import.meta.url),
  );
  const match = chartYaml.match(/\nversion:\s*([^\n]+)/);
  if (!match) {
    throw new Error("chart version was not found in charts/ugoite/Chart.yaml");
  }
  return match[1].trim().replace(/^"|"$/g, "");
}

async function run(
  cmd: string,
  args: string[],
): Promise<{ stdout: string; stderr: string }> {
  const child = new Deno.Command(cmd, {
    args,
    cwd: pathFromFileUrl(repoRoot),
    stdout: "piped",
    stderr: "piped",
  });
  const output = await child.output();
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

async function readTarGzEntry(
  archivePath: string,
  entryPath: string,
): Promise<string> {
  const result = await run("tar", [
    "-xOzf",
    archivePath,
    entryPath,
  ]);
  return result.stdout;
}
