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

export type E2eBundle = {
  cliArchivePath: string;
  imageArchivePath: string;
};

const SHA = /^[0-9a-f]{40}$/i;

if (import.meta.main) await main();

async function main(): Promise<void> {
  if (Deno.args[0] !== "load") {
    throw new Error("usage: deno run -A tools/ci_artifact_bundle.ts load");
  }
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
  );
  await writeOutput("verification_seconds", elapsedSeconds(verifyStarted));

  const cliOutput = "target/rust/release";
  await Deno.mkdir(cliOutput, { recursive: true });
  const extractStarted = Date.now();
  await run("tar", [
    "-xzf",
    bundle.cliArchivePath,
    "-C",
    cliOutput,
    "--no-same-owner",
  ]);
  const cliBinary = `${cliOutput}/ugoite`;
  await Deno.chmod(cliBinary, 0o755);
  await Deno.writeTextFile(`${cliBinary}.source-sha`, `${expectedSha}\n`);
  await writeOutput("cli_extract_seconds", elapsedSeconds(extractStarted));
  const imageLoadStarted = Date.now();
  await run("docker", ["load", "--input", bundle.imageArchivePath]);
  await writeOutput("image_load_seconds", elapsedSeconds(imageLoadStarted));

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
): Promise<E2eBundle> {
  if (!SHA.test(expectedSourceSha)) {
    throw new Error("expected source SHA must be a 40-character Git SHA");
  }
  if (!expectedCiRunId) throw new Error("expected CI run ID is required");
  if (checkoutSourceSha !== expectedSourceSha) {
    throw new Error("artifact source SHA does not match the E2E checkout");
  }

  const root = await Deno.realPath(artifactRoot);
  const manifestPath = `${root}/manifest.json`;
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

  const verified = new Map<string, ManifestFile[]>();
  for (const kind of ["cli", "image"] as const) {
    const artifact = manifest.artifacts.find((item) => item.kind === kind);
    if (!artifact || artifact.files.length === 0) {
      throw new Error(`artifact manifest is missing ${kind} files`);
    }
    for (const file of artifact.files) {
      const filePath = await containedArtifactPath(root, file.path);
      const bytes = await Deno.readFile(filePath);
      const digest = await sha256(bytes);
      if (digest !== file.sha256) {
        throw new Error(`artifact checksum mismatch for ${file.path}`);
      }
      if (bytes.byteLength !== file.size) {
        throw new Error(`artifact size mismatch for ${file.path}`);
      }
    }
    verified.set(kind, artifact.files);
  }

  const cliArchive = verified.get("cli")?.find((file) =>
    file.path.endsWith(".tar.gz")
  );
  const imageArchive = verified.get("image")?.find((file) =>
    file.path.endsWith(".tar.gz")
  );
  if (!cliArchive) throw new Error("verified CLI archive is missing");
  if (!imageArchive) {
    throw new Error("verified runtime image archive is missing");
  }

  return {
    cliArchivePath: await containedArtifactPath(root, cliArchive.path),
    imageArchivePath: await containedArtifactPath(root, imageArchive.path),
  };
}

async function containedArtifactPath(root: string, relativePath: string) {
  if (
    relativePath.length === 0 || relativePath.startsWith("/") ||
    relativePath.includes("\\") ||
    relativePath.split("/").some((part) =>
      part === "" || part === "." || part === ".."
    )
  ) {
    throw new Error(`unsafe artifact manifest path: ${relativePath}`);
  }
  const resolved = await Deno.realPath(`${root}/${relativePath}`);
  if (!resolved.startsWith(`${root}/`)) {
    throw new Error(`artifact path escapes bundle root: ${relativePath}`);
  }
  return resolved;
}

async function sha256(bytes: Uint8Array): Promise<string> {
  const buffer = new ArrayBuffer(bytes.byteLength);
  new Uint8Array(buffer).set(bytes);
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return [...new Uint8Array(digest)].map((value) =>
    value.toString(16).padStart(2, "0")
  ).join("");
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
