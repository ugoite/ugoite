import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export type ReleaseNotesOptions = {
  version: string;
  repoRoot?: string;
  sourcePath?: string;
};

const RELEASE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const REPO_ROOT = fileURLToPath(new URL("../", import.meta.url));

if (import.meta.main) {
  await main(Deno.args);
}

/**
 * Return the repository-relative path for one stable release note.
 *
 * Release notes are authored Markdown, not a rendered projection of channel
 * YAML. Keeping this path calculation in one validator makes candidate and
 * publish workflows agree on the exact source file without introducing a
 * second release-note authority.
 */
export function releaseNotePath(
  version: string,
  repoRoot = REPO_ROOT,
): string {
  assertStableVersion(version);
  return resolve(repoRoot, "docs/version/releases", `v${version}.md`);
}

export async function validateReleaseNote(
  options: ReleaseNotesOptions,
): Promise<string> {
  assertStableVersion(options.version);
  const repoRoot = resolve(options.repoRoot ?? REPO_ROOT);
  const sourcePath = resolve(
    options.sourcePath ?? releaseNotePath(options.version, repoRoot),
  );
  const note = await readFile(sourcePath);
  const frontmatterTitle = releaseTitle(note);
  const firstNonEmptyLine = stripFrontmatter(note)
    .split(/\r?\n/)
    .find((line) => line.trim());
  if (!frontmatterTitle && !firstNonEmptyLine) {
    throw new Error(`release note is empty at ${sourcePath}`);
  }

  const heading = frontmatterTitle ??
    (firstNonEmptyLine
      ? /^#\s+Ugoite\s+v(\d+\.\d+\.\d+)(?:\s|$)/.exec(
        firstNonEmptyLine.trim(),
      )
      : null);
  if (!heading) {
    throw new Error(
      `release note must contain a versioned frontmatter title or heading for ${options.version}`,
    );
  }
  if (heading[1] !== options.version) {
    throw new Error(
      `release note heading version ${
        heading[1]
      } does not match ${options.version}`,
    );
  }
  return note;
}

function stripFrontmatter(note: string): string {
  const frontmatter = /^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/.exec(note);
  return frontmatter ? note.slice(frontmatter[0].length) : note;
}

function releaseTitle(
  note: string,
): RegExpExecArray | null {
  const frontmatter = /^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(note);
  if (!frontmatter) return null;
  return /^title:\s*["']?Ugoite\s+v(\d+\.\d+\.\d+)(?:\s|["']|$)/m.exec(
    frontmatter[1],
  );
}

function assertStableVersion(version: string): void {
  if (!RELEASE_VERSION.test(version)) {
    throw new Error(
      `release version must be stable SemVer x.y.z, got ${version}`,
    );
  }
}

async function main(args: string[]): Promise<void> {
  const command = args[0];
  const flags = parseFlags(args.slice(1));
  if (command !== "validate") {
    throw new Error(
      "usage: release-notes.ts validate --version <version> [--source-path <path>] [--output <path>]",
    );
  }

  const version = requireFlag(flags, "version");
  const note = await validateReleaseNote({
    version,
    sourcePath: flags["source-path"],
  });
  await writeResult(note, flags.output);
}

function parseFlags(args: string[]): Record<string, string> {
  const flags: Record<string, string> = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) {
      throw new Error(`unexpected argument: ${arg}`);
    }
    const key = arg.slice(2);
    const value = args[++index];
    if (!value || value.startsWith("--")) {
      throw new Error(`flag --${key} requires a value`);
    }
    flags[key] = value;
  }
  return flags;
}

function requireFlag(flags: Record<string, string>, key: string): string {
  const value = flags[key]?.trim();
  if (!value) {
    throw new Error(`missing required flag --${key}`);
  }
  return value;
}

async function readFile(path: string): Promise<string> {
  try {
    return await Deno.readTextFile(path);
  } catch {
    throw new Error(`release note was not found at ${path}`);
  }
}

async function writeResult(
  content: string,
  outputPath: string | undefined,
): Promise<void> {
  if (outputPath) {
    await Deno.writeTextFile(outputPath, content);
    return;
  }
  console.log(content.trim());
}
