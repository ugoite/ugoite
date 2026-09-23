/**
 * Repository-owned Mitase 0.2.x release sync.
 *
 * Usage:
 *   deno task mitase:sync --version 0.2.N --sha256 <target>=<digest> [--sha256 ...]
 *
 * The tool performs exactly one deterministic operation:
 * 1. Fetches the official immutable release archives for the exact version
 *    (all four canonical targets; never a branch, HEAD, or mutable
 *    `latest` URL).
 * 2. Fails without changing any repository file when a supplied SHA-256
 *    does not match.
 * 3. Verifies the archive shape (exactly one `mitase` executable entry)
 *    and, for the host target, that `mitase --version` reports the exact
 *    version.
 * 4. Rewrites tools/mitase.lock.toml (version + digest pins) atomically.
 *
 * Only immutable 0.2.x releases are accepted. A Mitase update never absorbs
 * Ugoite product-semantics, Space-version, or Knowledge-encoding changes:
 * those stay visible through the existing suites (mitase check, capability
 * projection, representative corpus), which this tool leaves runnable but
 * does not execute, own, or plan.
 */

const CANONICAL_TARGETS = [
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
] as const;

export type CanonicalTarget = (typeof CANONICAL_TARGETS)[number];

const DEFAULT_RELEASE_BASE_URL =
  "https://github.com/ugoite/mitase/releases/download";

const VERSION_PATTERN = /^(\d+)\.(\d+)\.(\d+)$/;
const DIGEST_PATTERN = /^[0-9a-f]{64}$/;

export type SyncRequest = {
  version: string;
  digests: Map<CanonicalTarget, string>;
  baseUrl: string;
  lockPath: string;
};

function fail(message: string): never {
  throw new Error(`mitase:sync: ${message}`);
}

function isCanonicalTarget(value: string): value is CanonicalTarget {
  return (CANONICAL_TARGETS as readonly string[]).includes(value);
}

/** Parses CLI args without touching the network or the filesystem. */
export function parseSyncArgs(
  args: string[],
  env: Record<string, string | undefined>,
  lockPath: string,
): SyncRequest {
  let version: string | undefined;
  const digests = new Map<CanonicalTarget, string>();
  for (let index = 0; index < args.length; index++) {
    const arg = args[index];
    if (arg === "--version") {
      version = args[++index];
    } else if (arg.startsWith("--version=")) {
      version = arg.slice("--version=".length);
    } else if (arg === "--sha256") {
      recordDigest(args[++index], digests);
    } else if (arg.startsWith("--sha256=")) {
      recordDigest(arg.slice("--sha256=".length), digests);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!version) {
    fail("missing required --version 0.2.N");
  }
  version = version.trim();
  validateSyncVersion(version);
  for (const target of CANONICAL_TARGETS) {
    if (!digests.has(target)) {
      fail(`missing required --sha256 ${target}=<digest>`);
    }
  }
  const baseUrl = env.MITASE_RELEASE_BASE_URL?.trim() ||
    DEFAULT_RELEASE_BASE_URL;
  if (/latest/i.test(baseUrl)) {
    fail("mutable release URLs are not accepted; pin an exact version");
  }
  return { version, digests, baseUrl, lockPath };
}

function recordDigest(
  spec: string | undefined,
  digests: Map<CanonicalTarget, string>,
): void {
  if (!spec) {
    fail("missing value for --sha256 <target>=<digest>");
  }
  const separator = spec.indexOf("=");
  const target = separator < 0 ? "" : spec.slice(0, separator);
  const digest = separator < 0 ? "" : spec.slice(separator + 1).toLowerCase();
  if (!isCanonicalTarget(target)) {
    fail(
      `--sha256 target must be one of ${CANONICAL_TARGETS.join(", ")}`,
    );
  }
  if (!DIGEST_PATTERN.test(digest)) {
    fail(`--sha256 digest for ${target} must be 64 lowercase hex characters`);
  }
  digests.set(target, digest);
}

/**
 * Only immutable 0.2.x releases may flow through this tool. Anything else
 * (HEAD, branches, other majors/minors, and the retired 0.1.x line) is
 * rejected so a Mitase update can never silently carry
 * product-semantics drift into the repository.
 */
export function validateSyncVersion(version: string): void {
  const match = VERSION_PATTERN.exec(version.trim());
  if (!match || match[1] !== "0" || match[2] !== "2") {
    fail(
      `refusing version ${JSON.stringify(version)}: only 0.2.N releases sync`,
    );
  }
}

export function archiveName(version: string, target: CanonicalTarget): string {
  return `mitase-v${version}-${target}.tar.gz`;
}

export function releaseArchiveUrl(
  baseUrl: string,
  version: string,
  target: CanonicalTarget,
): string {
  return `${baseUrl.replace(/\/$/, "")}/v${version}/${
    archiveName(version, target)
  }`;
}

/** Deterministic lock rendering matching the checked-in lock layout. */
export function renderLock(request: SyncRequest): string {
  const sections = CANONICAL_TARGETS.map((target) =>
    `[target.${target}]\nsha256 = "${request.digests.get(target)}"`
  );
  return `version = "${request.version}"\n\n${sections.join("\n\n")}\n`;
}

export async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const view = new Uint8Array(bytes);
  const digest = await crypto.subtle.digest("SHA-256", view.buffer);
  return [...new Uint8Array(digest)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

async function runCommand(
  command: string,
  args: string[],
): Promise<{ success: boolean; stdout: string; stderr: string }> {
  const process = new Deno.Command(command, {
    args,
    stdout: "piped",
    stderr: "piped",
  });
  const output = await process.output();
  const decoder = new TextDecoder();
  return {
    success: output.success,
    stdout: decoder.decode(output.stdout),
    stderr: decoder.decode(output.stderr),
  };
}

/**
 * Verifies one downloaded archive against its supplied digest and release
 * shape. Nothing is written on failure. Returns the extracted directory
 * holding the single `mitase` entry.
 */
export async function verifyArchive(
  archive: Uint8Array,
  request: SyncRequest,
  target: CanonicalTarget,
  workDir: string,
): Promise<string> {
  const expected = request.digests.get(target) ?? fail("unreachable");
  const actual = await sha256Hex(archive);
  if (actual !== expected) {
    fail(
      `SHA-256 mismatch for ${archiveName(request.version, target)}: ` +
        `expected ${expected}, got ${actual}; repository left unchanged`,
    );
  }
  const archivePath = `${workDir}/${archiveName(request.version, target)}`;
  await Deno.writeFile(archivePath, archive);
  const listed = await runCommand("tar", ["-tzf", archivePath]);
  if (!listed.success) {
    fail(
      `cannot list ${
        archiveName(request.version, target)
      }: ${listed.stderr.trim()}`,
    );
  }
  const entries = listed.stdout.split("\n").filter((line) => line.length > 0);
  if (entries.length !== 1 || entries[0] !== "mitase") {
    fail(
      `release archive must contain exactly one mitase entry, got: ${
        JSON.stringify(entries)
      }`,
    );
  }
  const extractDir = `${workDir}/extracted-${target}`;
  await Deno.mkdir(extractDir, { recursive: true });
  const extracted = await runCommand("tar", [
    "-xzf",
    archivePath,
    "-C",
    extractDir,
  ]);
  if (!extracted.success) {
    fail(
      `cannot extract ${
        archiveName(request.version, target)
      }: ${extracted.stderr.trim()}`,
    );
  }
  const binary = `${extractDir}/mitase`;
  const stat = await Deno.stat(binary).catch(() =>
    fail("archive has no mitase binary")
  );
  if (stat.mode !== null && (stat.mode & 0o111) === 0) {
    fail("release mitase entry is not executable");
  }
  return extractDir;
}

function hostTarget(): CanonicalTarget {
  if (Deno.build.os === "darwin") {
    return Deno.build.arch === "aarch64"
      ? "aarch64-apple-darwin"
      : "x86_64-apple-darwin";
  }
  return Deno.build.arch === "aarch64"
    ? "aarch64-unknown-linux-gnu"
    : "x86_64-unknown-linux-gnu";
}

if (import.meta.main) await main(Deno.args, Deno.env.toObject());

export async function main(
  args: string[],
  env: Record<string, string | undefined>,
): Promise<void> {
  const repoRoot = new URL("../", import.meta.url).pathname.replace(/\/$/, "");
  const request = parseSyncArgs(
    args,
    env,
    `${repoRoot}/tools/mitase.lock.toml`,
  );
  const workDir = await Deno.makeTempDir({ prefix: "ugoite-mitase-sync." });
  try {
    // Verify every target before writing anything: a single mismatch
    // fails the whole sync with the repository left unchanged.
    for (const target of CANONICAL_TARGETS) {
      const url = releaseArchiveUrl(request.baseUrl, request.version, target);
      console.log(`fetch ${url}`);
      const response = await fetch(url);
      if (!response.ok) {
        fail(`fetch failed (${response.status}) for ${url}`);
      }
      const archive = new Uint8Array(await response.arrayBuffer());
      await verifyArchive(archive, request, target, workDir);
    }
    // Executable check for the host target only: foreign-arch binaries
    // cannot run here, and their shape was verified above.
    const host = hostTarget();
    const probed = await runCommand(`${workDir}/extracted-${host}/mitase`, [
      "--version",
    ]);
    const reported = probed.stdout.trim();
    if (!probed.success || reported !== `mitase ${request.version}`) {
      fail(
        `host executable reports ${
          JSON.stringify(reported)
        }, expected "mitase ${request.version}"`,
      );
    }
    const rendered = renderLock(request);
    const tempPath = `${repoRoot}/tools/mitase.lock.toml.tmp`;
    await Deno.writeTextFile(tempPath, rendered);
    await Deno.rename(tempPath, request.lockPath);
    console.log(`updated ${request.lockPath} to Mitase ${request.version}`);
    // Leave the repository runnable through the pinned bootstrap.
    const smoke = await runCommand(`${repoRoot}/scripts/mitase`, ["--version"]);
    if (!smoke.success || smoke.stdout.trim() !== `mitase ${request.version}`) {
      fail(`post-sync bootstrap smoke failed: ${smoke.stderr.trim()}`);
    }
    console.log(smoke.stdout.trim());
  } finally {
    await Deno.remove(workDir, { recursive: true }).catch(() => {});
  }
}
