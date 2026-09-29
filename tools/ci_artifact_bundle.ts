import { containedArtifactPath, sha256File } from "./ci_file_security.ts";

type ManifestFile = {
  path: string;
  sha256: string;
  size: number;
};

type ManifestArtifact = {
  kind: string;
  files: ManifestFile[];
};

type ArtifactManifest = {
  schema_version: number;
  source_sha: string | null;
  ci_run_id: string | null;
  artifacts: ManifestArtifact[];
};

export type ArtifactSelection = "runtime" | "cli" | "both";

export type E2eBundle = {
  cliArchivePath: string | null;
  imageArchivePath: string | null;
};

const SHA = /^[0-9a-f]{40}$/i;

if (import.meta.main) await main();

async function main(): Promise<void> {
  if (Deno.args[0] !== "load" || Deno.args.length > 2) {
    throw new Error(
      "usage: deno run -A tools/ci_artifact_bundle.ts load [runtime|cli|both]",
    );
  }
  const selection = parseArtifactSelection(
    Deno.args[1] ?? Deno.env.get("UGOITE_ARTIFACT_SELECTION"),
  );
  const checkoutSha = await gitHead();
  const expectedSha = Deno.env.get("UGOITE_SOURCE_SHA")?.trim() || checkoutSha;
  const expectedRunId = Deno.env.get("UGOITE_CI_RUN_ID")?.trim() ||
    Deno.env.get("GITHUB_RUN_ID")?.trim() || "";
  const artifactRoot = Deno.env.get("UGOITE_ARTIFACT_ROOT")?.trim() ||
    "target/artifacts";
  const verifyStarted = Date.now();
  const bundle = await verifyE2eBundle(
    artifactRoot,
    expectedSha,
    expectedRunId,
    checkoutSha,
    selection,
  );
  await writeOutput("verification_seconds", elapsedSeconds(verifyStarted));

  if (bundle.cliArchivePath !== null) {
    const cliOutput = "target/rust/release";
    await Deno.mkdir(cliOutput, { recursive: true });
    const extractStarted = Date.now();
    await verifyCliArchiveMembers(bundle.cliArchivePath);
    const stagingDir = await Deno.makeTempDir({
      dir: cliOutput,
      prefix: ".ugoite-ci-cli-",
    });
    try {
      await run("tar", [
        "-xzf",
        bundle.cliArchivePath,
        "-C",
        stagingDir,
        "--no-same-owner",
      ]);
      const stagedBinary = `${stagingDir}/ugoite`;
      const stagedInfo = await Deno.lstat(stagedBinary);
      if (!stagedInfo.isFile || stagedInfo.isSymlink) {
        throw new Error(
          "verified CLI archive did not produce a regular binary",
        );
      }
      const cliBinary = `${cliOutput}/ugoite`;
      await Deno.chmod(stagedBinary, 0o755);
      await Deno.rename(stagedBinary, cliBinary);
      await Deno.writeTextFile(`${cliBinary}.source-sha`, `${expectedSha}\n`);
    } finally {
      await Deno.remove(stagingDir, { recursive: true }).catch(() => {});
    }
    await writeOutput("cli_extract_seconds", elapsedSeconds(extractStarted));
  }

  if (bundle.imageArchivePath !== null) {
    const imageLoadStarted = Date.now();
    await run("docker", ["load", "--input", bundle.imageArchivePath]);
    await writeOutput("image_load_seconds", elapsedSeconds(imageLoadStarted));
  }

  console.log(
    `Loaded verified E2E artifacts for source ${expectedSha} from CI run ${expectedRunId}`,
  );
}

function elapsedSeconds(started: number): number {
  return Math.ceil((Date.now() - started) / 1000);
}

async function writeOutput(key: string, value: number): Promise<void> {
  const outputPath = Deno.env.get("GITHUB_OUTPUT");
  if (outputPath) {
    await Deno.writeTextFile(outputPath, `${key}=${value}\n`, { append: true });
  }
}

export async function verifyE2eBundle(
  artifactRoot: string,
  expectedSourceSha: string,
  expectedCiRunId: string,
  checkoutSourceSha = expectedSourceSha,
  selection: ArtifactSelection = "both",
): Promise<E2eBundle> {
  if (selection !== "runtime" && selection !== "cli" && selection !== "both") {
    throw new Error(`unknown CI artifact selection ${selection}`);
  }
  if (!SHA.test(expectedSourceSha)) {
    throw new Error("expected source SHA must be a 40-character Git SHA");
  }
  if (!expectedCiRunId) throw new Error("expected CI run ID is required");
  if (checkoutSourceSha !== expectedSourceSha) {
    throw new Error("artifact source SHA does not match the E2E checkout");
  }

  const root = await Deno.realPath(artifactRoot);
  const manifestPath = await containedArtifactPath(root, "manifest.json");
  const manifest = JSON.parse(
    await Deno.readTextFile(manifestPath),
  ) as ArtifactManifest;
  if (manifest.schema_version !== 2) {
    throw new Error(
      `unsupported artifact manifest schema ${manifest.schema_version}`,
    );
  }
  if (manifest.source_sha !== expectedSourceSha) {
    throw new Error("artifact manifest source SHA does not match checkout");
  }
  if (manifest.ci_run_id !== expectedCiRunId) {
    throw new Error("artifact manifest CI run ID does not match workflow run");
  }
  if (!Array.isArray(manifest.artifacts)) {
    throw new Error("artifact manifest must contain an artifacts array");
  }

  const cliFiles = selection === "runtime"
    ? null
    : await verifyManifestArtifact(root, manifest.artifacts, "cli");
  const imageFiles = selection === "cli"
    ? null
    : await verifyManifestArtifact(root, manifest.artifacts, "image");
  const cliArchive = cliFiles?.find((file) => file.path.endsWith(".tar.gz"));
  const imageArchive = imageFiles?.find((file) =>
    file.path.endsWith(".tar.gz")
  );
  if (selection !== "runtime" && !cliArchive) {
    throw new Error("verified CLI archive is missing");
  }
  if (selection !== "cli" && !imageArchive) {
    throw new Error("verified runtime image archive is missing");
  }

  return {
    cliArchivePath: cliArchive
      ? await containedArtifactPath(root, cliArchive.path)
      : null,
    imageArchivePath: imageArchive
      ? await containedArtifactPath(root, imageArchive.path)
      : null,
  };
}

async function verifyManifestArtifact(
  root: string,
  artifacts: ManifestArtifact[],
  kind: "cli" | "image",
): Promise<ManifestFile[]> {
  const matches = artifacts.filter((item) => item.kind === kind);
  if (matches.length !== 1 || !Array.isArray(matches[0]?.files)) {
    throw new Error(`artifact manifest must contain exactly one ${kind} entry`);
  }
  const files = matches[0].files;
  if (files.length === 0) {
    throw new Error(`artifact manifest is missing ${kind} files`);
  }
  for (const file of files) {
    if (
      !Number.isSafeInteger(file.size) || file.size < 0 ||
      !/^[0-9a-f]{64}$/i.test(file.sha256)
    ) {
      throw new Error(
        `artifact manifest has invalid metadata for ${file.path}`,
      );
    }
    const filePath = await containedArtifactPath(root, file.path);
    const actual = await sha256File(filePath);
    if (actual.sha256 !== file.sha256) {
      throw new Error(`artifact checksum mismatch for ${file.path}`);
    }
    if (actual.size !== file.size) {
      throw new Error(`artifact size mismatch for ${file.path}`);
    }
  }
  return files;
}

export function parseArtifactSelection(
  raw: string | null | undefined,
): ArtifactSelection {
  const selection = raw?.trim() || "both";
  if (selection === "runtime" || selection === "cli" || selection === "both") {
    return selection;
  }
  throw new Error(`unknown CI artifact selection ${selection}`);
}

async function verifyCliArchiveMembers(archivePath: string): Promise<void> {
  const members = await run("tar", ["-tzf", archivePath]);
  const details = await run("tar", ["-tvzf", archivePath]);
  validateCliArchiveMembers(members, details);
}

export function validateCliArchiveMembers(
  members: string,
  details: string,
): void {
  const names = members.split(/\r?\n/).filter((name) => name.length > 0);
  const detailLines = details.split(/\r?\n/).filter((line) => line.length > 0);
  if (
    names.length !== 1 || names[0] !== "ugoite" || detailLines.length !== 1 ||
    !detailLines[0].startsWith("-") || !detailLines[0].endsWith(" ugoite")
  ) {
    throw new Error("CLI archive must contain only a regular ugoite binary");
  }
}

async function gitHead(): Promise<string> {
  return (await run("git", ["rev-parse", "HEAD"])).trim();
}

async function run(command: string, args: string[]): Promise<string> {
  const result = await new Deno.Command(command, {
    args,
    stdout: "piped",
    stderr: "piped",
  }).output();
  const stdout = new TextDecoder().decode(result.stdout).trim();
  const stderr = new TextDecoder().decode(result.stderr).trim();
  if (!result.success) {
    throw new Error(
      `${command} ${args.join(" ")} failed${stderr ? `: ${stderr}` : ""}`,
    );
  }
  return stdout;
}
