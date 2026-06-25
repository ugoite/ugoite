#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

bash scripts/check-placeholder-artifacts.sh

deno eval '
const maxTrackedFileBytes = 1024 * 1024;
const forbiddenSegments = new Set(["node_modules", "target"]);
const allowedLargeTrackedPaths = new Set([]);

const decoder = new TextDecoder();

async function command(args) {
  const output = await new Deno.Command(args[0], {
    args: args.slice(1),
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!output.success) {
    const stderr = decoder.decode(output.stderr).trim();
    throw new Error(`${args.join(" ")} failed${stderr ? `: ${stderr}` : ""}`);
  }
  return decoder.decode(output.stdout);
}

function formatBytes(size) {
  const units = ["bytes", "KiB", "MiB", "GiB"];
  let value = size;
  let unit = units[0];
  for (const candidate of units) {
    unit = candidate;
    if (value < 1024 || candidate === units.at(-1)) break;
    value /= 1024;
  }
  return unit === "bytes" ? `${Math.trunc(value)} ${unit}` : `${value.toFixed(1)} ${unit}`;
}

const trackedPaths = (await command(["git", "ls-files", "-z"])).split("\0").filter(Boolean);
const trackedIgnored = [];
const forbiddenPaths = [];
const oversizedPaths = [];

for (const rawPath of trackedPaths) {
  const ignored = await new Deno.Command("git", {
    args: ["check-ignore", "--no-index", rawPath],
    stdout: "null",
    stderr: "null",
  }).output();
  if (ignored.success) trackedIgnored.push(rawPath);

  const forbiddenSegment = rawPath.split("/").find((segment) => forbiddenSegments.has(segment));
  if (forbiddenSegment) {
    forbiddenPaths.push([rawPath, forbiddenSegment]);
    continue;
  }

  let stat;
  try {
    stat = await Deno.stat(rawPath);
  } catch (error) {
    if (error instanceof Deno.errors.NotFound) continue;
    throw error;
  }
  if (
    stat.size > maxTrackedFileBytes &&
    !allowedLargeTrackedPaths.has(rawPath)
  ) {
    oversizedPaths.push([rawPath, stat.size]);
  }
}

const messages = [];
if (trackedIgnored.length > 0) {
  messages.push([
    "Tracked files must not also match ignore rules:",
    ...trackedIgnored.sort().map((path) => `  - ${path}`),
    "",
    "Remove these files from git or adjust .gitignore before committing.",
  ].join("\n"));
}
if (forbiddenPaths.length > 0) {
  messages.push([
    "Tracked files must not live in generated dependency/build directories:",
    ...forbiddenPaths
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([path, segment]) => `  - ${path} (contains ${segment})`),
    "",
    "Generated dependency/build directories belong in local caches, not in source control.",
  ].join("\n"));
}
if (oversizedPaths.length > 0) {
  messages.push([
    `Tracked files must stay below ${formatBytes(maxTrackedFileBytes)} unless allowlisted:`,
    ...oversizedPaths
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([path, size]) => `  - ${path} (${formatBytes(size)})`),
    "",
    "Large binaries, generated bundles, and packaged artifacts should be published as release assets instead.",
  ].join("\n"));
}

if (messages.length > 0) {
  console.error(messages.join("\n\n"));
  Deno.exit(1);
}
'

echo "Repository root artifact hygiene check passed."
