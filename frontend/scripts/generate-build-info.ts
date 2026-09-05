const sourceSha = Deno.env.get("UGOITE_SOURCE_SHA")?.trim() ||
  await resolveGitSourceSha();
const outputPath = Deno.args[0] ?? ".output/public/build-info.json";
const outputDirectory = outputPath.slice(0, outputPath.lastIndexOf("/")) || ".";

if (!sourceSha || !/^(?:[0-9a-f]{40}|unknown)$/.test(sourceSha)) {
  throw new Error(
    `UGOITE_SOURCE_SHA must be a 40-character Git SHA or unknown, got ${
      sourceSha ?? "<missing>"
    }`,
  );
}

await Deno.mkdir(outputDirectory, { recursive: true });
await Deno.writeTextFile(
  outputPath,
  `${JSON.stringify({ schema_version: 1, source_sha: sourceSha }, null, 2)}\n`,
);

async function resolveGitSourceSha(): Promise<string> {
  try {
    const output = await new Deno.Command("git", {
      args: ["-C", "..", "rev-parse", "HEAD"],
      stdout: "piped",
      stderr: "null",
    }).output();
    if (output.success) {
      return new TextDecoder().decode(output.stdout).trim();
    }
  } catch {
    // Source archives and container build contexts may not contain .git.
  }
  return "unknown";
}
