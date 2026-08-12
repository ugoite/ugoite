import { parse } from "yaml";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export type ReleaseChannel = "stable" | "beta" | "alpha";

export type ReleaseNotesOptions = {
  channel: ReleaseChannel;
  version: string;
  repoRoot?: string;
  sourcePath?: string;
};

export type ComposeReleaseNotesOptions = ReleaseNotesOptions & {
  existingBody: string;
  channelNotes?: string;
};

const RELEASE_VERSION = /^\d+\.\d+\.\d+(?:-(alpha|beta)\.\d+)?$/;
const CHANNELS = ["stable", "beta", "alpha"] as const;
const CHANNEL_NOTES_START = "<!-- UGOITE-CHANNEL-NOTES:v1:start";
const CHANNEL_NOTES_END = "<!-- UGOITE-CHANNEL-NOTES:v1:end -->";
const REPO_ROOT = fileURLToPath(new URL("../", import.meta.url));

if (import.meta.main) {
  await main(Deno.args);
}

export function channelForVersion(version: string): ReleaseChannel {
  const match = RELEASE_VERSION.exec(version);
  if (!match) {
    throw new Error(`release version must be valid SemVer, got ${version}`);
  }
  return (match[1] ?? "stable") as ReleaseChannel;
}

export async function renderReleaseNotes(
  options: ReleaseNotesOptions,
): Promise<string> {
  assertChannelMatchesVersion(options.channel, options.version);
  const repoRoot = resolve(options.repoRoot ?? REPO_ROOT);
  const sourcePath = resolve(
    options.sourcePath ??
      join(repoRoot, "docs/version/changelog", `${options.channel}.yaml`),
  );
  const source = await readFile(sourcePath, "channel changelog source");
  const document = parseChannelDocument(source, sourcePath);
  const context = relative(repoRoot, sourcePath) || sourcePath;
  const configuredChannel = requiredString(document, "channel", context);
  if (configuredChannel !== options.channel) {
    throw new Error(
      `${context} channel must be ${options.channel}, got ${configuredChannel}`,
    );
  }

  const title = requiredString(document, "title", context);
  const docPath = requiredString(document, "doc_path", context);
  const fullDocPath = resolve(repoRoot, docPath);
  if (relative(repoRoot, fullDocPath).startsWith("..")) {
    throw new Error(`${context} doc_path escapes the repository: ${docPath}`);
  }
  await readFile(fullDocPath, "human-readable changelog document");
  const summary = requiredString(document, "summary", context);
  const releaseNotes = requiredMapping(document, "release_notes", context);
  const releaseNotesContext = `${context}.release_notes`;
  const intro = requiredString(releaseNotes, "intro", releaseNotesContext);
  const expectations = requiredStringList(
    releaseNotes,
    "expectations",
    releaseNotesContext,
  );
  const added = requiredStringList(releaseNotes, "added", releaseNotesContext);
  const changed = requiredStringList(
    releaseNotes,
    "changed",
    releaseNotesContext,
  );
  const planned = requiredStringList(
    releaseNotes,
    "planned",
    releaseNotesContext,
  );

  return [
    `# v${options.version} ${title}`,
    `Rendered from \`docs/version/changelog/${options.channel}.yaml\` for the \`${options.channel}\` release channel. Human-readable changelog: \`${docPath}\`.`,
    summary,
    `## Channel guidance\n\n${intro}`,
    renderSection("Expectations", expectations),
    renderSection("Added", added),
    renderSection("Changed", changed),
    renderSection("Planned", planned),
  ].join("\n\n");
}

export function composeReleaseNotes(
  options: ComposeReleaseNotesOptions,
): string {
  assertChannelMatchesVersion(options.channel, options.version);
  const channelNotes = options.channelNotes?.trim();
  if (!channelNotes) {
    throw new Error("channel notes must be non-empty");
  }

  const start =
    `${CHANNEL_NOTES_START} channel=${options.channel} version=${options.version} -->`;
  const existingBody = options.existingBody.trim();
  const startIndex = existingBody.indexOf(CHANNEL_NOTES_START);
  const endIndex = existingBody.indexOf(CHANNEL_NOTES_END);
  if (startIndex === -1 && endIndex === -1) {
    return `${start}\n${channelNotes}\n${CHANNEL_NOTES_END}${
      existingBody ? `\n\n${existingBody}` : ""
    }\n`;
  }
  if (startIndex === -1 || endIndex === -1) {
    throw new Error("release body contains an incomplete channel-notes marker");
  }
  if (
    startIndex !== existingBody.lastIndexOf(CHANNEL_NOTES_START) ||
    endIndex !== existingBody.lastIndexOf(CHANNEL_NOTES_END) ||
    endIndex < startIndex
  ) {
    throw new Error(
      "release body contains multiple or out-of-order channel-notes markers",
    );
  }
  const existingStart = existingBody.slice(
    startIndex,
    existingBody.indexOf("-->", startIndex) + 3,
  );
  if (existingStart !== start) {
    throw new Error(
      "release body channel-notes marker does not match the release channel or version",
    );
  }
  const endAfterMarker = endIndex + CHANNEL_NOTES_END.length;
  const replacement = `${start}\n${channelNotes}\n${CHANNEL_NOTES_END}`;
  return `${existingBody.slice(0, startIndex)}${replacement}${
    existingBody.slice(endAfterMarker)
  }\n`;
}

async function main(args: string[]): Promise<void> {
  const command = args[0];
  const flags = parseFlags(args.slice(1));
  const channel = requireFlag(flags, "channel") as ReleaseChannel;
  if (!CHANNELS.includes(channel)) {
    throw new Error(
      `channel must be one of ${CHANNELS.join(", ")}, got ${channel}`,
    );
  }
  const version = requireFlag(flags, "version");

  if (command === "render") {
    const rendered = await renderReleaseNotes({ channel, version });
    await writeResult(rendered, flags.output);
    return;
  }
  if (command === "compose") {
    const bodyPath = requireFlag(flags, "body-file");
    const body = await Deno.readTextFile(bodyPath);
    const channelNotes = await renderReleaseNotes({ channel, version });
    const composed = composeReleaseNotes({
      channel,
      version,
      existingBody: body,
      channelNotes,
    });
    await writeResult(composed, flags.output);
    return;
  }
  throw new Error(
    "usage: release-notes.ts <render|compose> --channel <channel> --version <version> [--body-file <path>] [--output <path>]",
  );
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

function assertChannelMatchesVersion(
  channel: ReleaseChannel,
  version: string,
): void {
  const derived = channelForVersion(version);
  if (derived !== channel) {
    throw new Error(
      `channel ${channel} does not match release version ${version}; expected ${derived}`,
    );
  }
}

function parseChannelDocument(
  source: string,
  sourcePath: string,
): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = parse(source);
  } catch (error) {
    throw new Error(`unable to parse ${sourcePath}: ${error}`);
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`${sourcePath} must be a YAML mapping`);
  }
  return parsed as Record<string, unknown>;
}

function requiredMapping(
  mapping: Record<string, unknown>,
  key: string,
  context: string,
): Record<string, unknown> {
  const value = mapping[key];
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${context}.${key} must be a mapping`);
  }
  return value as Record<string, unknown>;
}

function requiredString(
  mapping: Record<string, unknown>,
  key: string,
  context: string,
): string {
  const value = mapping[key];
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`${context}.${key} must be a non-empty string`);
  }
  return value.trim();
}

function requiredStringList(
  mapping: Record<string, unknown>,
  key: string,
  context: string,
): string[] {
  const value = mapping[key];
  if (
    !Array.isArray(value) || value.length === 0 ||
    value.some((item) => typeof item !== "string" || !item.trim())
  ) {
    throw new Error(`${context}.${key} must be a non-empty list of strings`);
  }
  return value.map((item) => (item as string).trim());
}

function renderSection(title: string, items: string[]): string {
  return `## ${title}\n\n${items.map((item) => `- ${item}`).join("\n")}`;
}

async function readFile(path: string, label: string): Promise<string> {
  try {
    return await Deno.readTextFile(path);
  } catch {
    throw new Error(`${label} was not found at ${path}`);
  }
}

async function writeResult(
  content: string,
  outputPath: string | undefined,
): Promise<void> {
  if (outputPath) {
    await Deno.writeTextFile(outputPath, `${content.trim()}\n`);
    return;
  }
  console.log(content.trim());
}
