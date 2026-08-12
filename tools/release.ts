const decoder = new TextDecoder();
const repoRoot = new URL("../", import.meta.url);
const artifactRoot = new URL("../target/artifacts/", import.meta.url);

type ReleaseChannel = "stable" | "beta" | "alpha";

type VersionState = {
  workspace: string;
  npmPackage: string;
  helmChart: string;
  helmApp: string;
  helmImageTag: string;
  versionFile: string;
  releasePleaseManifest: string;
  npmPackageName: string;
  npmRegistry: string;
};

type ReleaseMetadata = {
  version: string;
  tag: string;
  channel: ReleaseChannel;
  prerelease: boolean;
  aliases: string[];
};

const command = Deno.args[0];

switch (command) {
  case "print-version":
    console.log((await readVersionState()).workspace);
    break;
  case "validate-release":
    await validateRelease();
    break;
  case "release-metadata":
    console.log(JSON.stringify(await buildReleaseMetadata(), null, 2));
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
  case "verify-npm":
    await verifyNpm();
    break;
  case "verify-helm":
    await verifyHelmPackage();
    break;
  default:
    throw new Error(
      "usage: deno run -A tools/release.ts <print-version|validate-release|release-metadata|package-cli|verify-cli|package-npm|verify-npm|verify-helm>",
    );
}

async function validateRelease(): Promise<void> {
  const state = await readVersionState();
  const versions = [
    state.workspace,
    state.npmPackage,
    state.helmChart,
    state.helmApp,
    state.helmImageTag,
    state.versionFile,
    state.releasePleaseManifest,
  ];
  const unique = [...new Set(versions)];
  if (unique.length !== 1) {
    throw new Error(
      `release versions disagree: ${JSON.stringify(state, null, 2)}`,
    );
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

  const metadata = buildMetadataFromVersion(state.workspace);
  if (!/^v\d+\.\d+\.\d+(-(?:alpha|beta)\.\d+)?$/.test(metadata.tag)) {
    throw new Error(`release tag must be v<SemVer>, got ${metadata.tag}`);
  }
}

async function packageCli(): Promise<void> {
  const metadata = await buildReleaseMetadata();
  const target = Deno.env.get("UGOITE_CLI_TARGET")?.trim();
  if (!target) {
    throw new Error("UGOITE_CLI_TARGET must be set");
  }
  const binaryPath = Deno.env.get("UGOITE_CLI_BINARY_PATH")?.trim() ??
    "target/rust/release/ugoite";
  await ensureFile(binaryPath, "ugoite CLI binary");
  await Deno.mkdir(new URL("cli/", artifactRoot), { recursive: true });

  const archiveName = cliArchiveName(metadata.tag, target);
  const archivePath = pathJoin("target/artifacts/cli", archiveName);
  const checksumPath = `${archivePath}.sha256`;
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-cli-package-" });
  const stagedBinary = `${tempDir}/ugoite`;
  await Deno.copyFile(binaryPath, stagedBinary);
  await run("tar", ["-C", tempDir, "-czf", archivePath, "ugoite"]);
  await writeChecksumFile(archivePath, checksumPath);
}

async function verifyCli(): Promise<void> {
  const metadata = await buildReleaseMetadata();
  const target = Deno.env.get("UGOITE_CLI_TARGET")?.trim();
  if (!target) {
    throw new Error("UGOITE_CLI_TARGET must be set");
  }
  const archivePath = pathJoin(
    "target/artifacts/cli",
    cliArchiveName(metadata.tag, target),
  );
  const checksumPath = `${archivePath}.sha256`;
  await ensureFile(archivePath, "CLI archive");
  await ensureFile(checksumPath, "CLI checksum");
  await verifyChecksumFile(archivePath, checksumPath);

  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-cli-verify-" });
  await run("tar", ["-xzf", archivePath, "-C", tempDir]);
  const cliPath = `${tempDir}/ugoite`;
  await ensureFile(cliPath, "extracted ugoite binary");
  const version = (await run(cliPath, ["--version"])).stdout.trim();
  if (!version.includes(metadata.version)) {
    throw new Error(
      `ugoite --version must contain ${metadata.version}, got ${version}`,
    );
  }
}

async function packageNpm(): Promise<void> {
  const packageDir = pathJoin("packages", "ugoite");
  const targetDir = pathJoin("target", "artifacts", "npm");
  await Deno.mkdir(targetDir, { recursive: true });
  const result = await run("npm", [
    "pack",
    "--json",
    "--pack-destination",
    pathJoin(pathFromFileUrl(repoRoot), targetDir),
  ], packageDir);
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

async function verifyNpm(): Promise<void> {
  const state = await readVersionState();
  const tarball = packageDirTarballName(state.npmPackage);
  const tarballPath = pathJoin("target", "artifacts", "npm", tarball);
  const checksumPath = `${tarballPath}.sha256`;
  await ensureFile(tarballPath, "npm package tarball");
  await ensureFile(checksumPath, "npm package checksum");
  await verifyChecksumFile(tarballPath, checksumPath);

  const inspect = JSON.parse(
    (await run("npm", [
      "pack",
      "--dry-run",
      "--json",
    ], pathJoin("packages", "ugoite"))).stdout,
  ) as Array<
    { name?: string; version?: string }
  >;
  if (inspect[0]?.name !== "@ugoite/ugoite") {
    throw new Error(
      `npm package dry-run resolved unexpected name ${inspect[0]?.name}`,
    );
  }
  if (inspect[0]?.version !== state.npmPackage) {
    throw new Error(
      `npm package dry-run resolved unexpected version ${inspect[0]?.version}`,
    );
  }
}

async function verifyHelmPackage(): Promise<void> {
  const state = await readVersionState();
  const archivePath = pathJoin(
    "target",
    "artifacts",
    "helm",
    `ugoite-${state.helmChart}.tgz`,
  );
  await ensureFile(archivePath, "Helm chart archive");
  const tempDir = await Deno.makeTempDir({ prefix: "ugoite-helm-verify-" });
  await run("tar", ["-xzf", archivePath, "-C", tempDir]);
  await run("helm", ["lint", `${tempDir}/ugoite`]);
  await run("helm", [
    "template",
    "ugoite",
    `${tempDir}/ugoite`,
    "--set",
    "nodeSecret.existingSecret=ugoite-node-secret",
  ]);
}

async function buildReleaseMetadata(): Promise<ReleaseMetadata> {
  const state = await readVersionState();
  return buildMetadataFromVersion(state.workspace);
}

function buildMetadataFromVersion(version: string): ReleaseMetadata {
  const prerelease = version.match(/-(alpha|beta)\.(\d+)$/);
  let channel: ReleaseChannel = "stable";
  let aliases = ["stable", "latest"];
  if (prerelease) {
    channel = prerelease[1] as ReleaseChannel;
    aliases = [channel];
  }
  return {
    version,
    tag: `v${version}`,
    channel,
    prerelease: channel !== "stable",
    aliases,
  };
}

async function readVersionState(): Promise<VersionState> {
  const cargoToml = await Deno.readTextFile(
    new URL("../Cargo.toml", import.meta.url),
  );
  const packageJson = JSON.parse(
    await Deno.readTextFile(
      new URL("../packages/ugoite/package.json", import.meta.url),
    ),
  ) as {
    name?: string;
    version?: string;
    publishConfig?: { registry?: string };
  };
  const chartYaml = await Deno.readTextFile(
    new URL("../charts/ugoite/Chart.yaml", import.meta.url),
  );
  const valuesYaml = await Deno.readTextFile(
    new URL("../charts/ugoite/values.yaml", import.meta.url),
  );
  const versionFile = (await Deno.readTextFile(
    new URL("../version.txt", import.meta.url),
  )).trim();
  const releaseManifest = JSON.parse(
    await Deno.readTextFile(
      new URL("../.release-please-manifest.json", import.meta.url),
    ),
  ) as Record<string, string>;

  return {
    workspace: capture(
      cargoToml,
      /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
      "workspace version",
    ),
    npmPackage: packageJson.version ??
      fail("packages/ugoite/package.json version is missing"),
    helmChart: capture(
      chartYaml,
      /\nversion:\s*([^\n]+)/,
      "Helm chart version",
    ),
    helmApp: capture(chartYaml, /\nappVersion:\s*([^\n]+)/, "Helm appVersion"),
    helmImageTag: capture(
      valuesYaml,
      /\n\x20\x20tag:\s*([^\n]+)/,
      "Helm image tag",
    ),
    versionFile,
    releasePleaseManifest: releaseManifest["."] ??
      fail('.release-please-manifest.json must define "."'),
    npmPackageName: packageJson.name ??
      fail("packages/ugoite/package.json name is missing"),
    npmRegistry: packageJson.publishConfig?.registry ??
      fail("packages/ugoite/package.json publishConfig.registry is missing"),
  };
}

function cliArchiveName(tag: string, target: string): string {
  return `ugoite-${tag}-${target}.tar.gz`;
}

function packageDirTarballName(version: string): string {
  return `ugoite-ugoite-${version}.tgz`;
}

function capture(text: string, pattern: RegExp, label: string): string {
  const match = text.match(pattern);
  if (!match) {
    fail(`${label} not found`);
  }
  return match[1].trim().replace(/\s+#.*$/, "").replace(/^"|"$/g, "");
}

async function ensureFile(path: string, label: string): Promise<void> {
  try {
    const stat = await Deno.stat(path);
    if (!stat.isFile) {
      throw new Error(`${label} must be a file: ${path}`);
    }
  } catch (error) {
    if (error instanceof Error) {
      throw new Error(`${label} was not found at ${path}`);
    }
    throw error;
  }
}

async function verifyChecksumFile(
  archivePath: string,
  checksumPath: string,
): Promise<void> {
  const expectedLine = (await Deno.readTextFile(checksumPath)).trim();
  const [expectedDigest, expectedFile] = expectedLine.split(/\s+/, 2);
  if (!expectedDigest || !expectedFile) {
    throw new Error(`invalid checksum file ${checksumPath}`);
  }
  const actualDigest = await sha256File(archivePath);
  const actualFile = basename(archivePath);
  if (expectedFile !== actualFile) {
    throw new Error(
      `checksum file ${checksumPath} recorded ${expectedFile}, expected ${actualFile}`,
    );
  }
  if (expectedDigest !== actualDigest) {
    throw new Error(`checksum mismatch for ${archivePath}`);
  }
}

async function writeChecksumFile(
  archivePath: string,
  checksumPath: string,
): Promise<void> {
  const digest = await sha256File(archivePath);
  await Deno.writeTextFile(
    checksumPath,
    `${digest}  ${basename(archivePath)}\n`,
  );
}

async function sha256File(path: string): Promise<string> {
  const bytes = await Deno.readFile(path);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((value) =>
    value.toString(16).padStart(2, "0")
  ).join("");
}

async function run(
  cmd: string,
  args: string[],
  cwd = pathFromFileUrl(repoRoot),
): Promise<{ stdout: string; stderr: string }> {
  const child = new Deno.Command(cmd, {
    args,
    cwd,
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

function basename(path: string): string {
  return path.split("/").at(-1) ?? path;
}

function pathJoin(...parts: string[]): string {
  return parts.join("/").replace(/\/+/g, "/");
}

function pathFromFileUrl(url: URL): string {
  return decodeURIComponent(url.pathname);
}

function fail(message: string): never {
  throw new Error(message);
}
