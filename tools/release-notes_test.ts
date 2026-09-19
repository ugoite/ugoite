import { assertEquals } from "@std/assert/equals";
import { releaseNotePath, validateReleaseNote } from "./release-notes.ts";

Deno.test("REQ-OPS-026: stable release notes use one versioned Markdown source", async () => {
  const repoRoot = await Deno.makeTempDir({ prefix: "ugoite-release-notes-" });
  const notePath = releaseNotePath("0.1.2", repoRoot);
  await Deno.mkdir(notePath.substring(0, notePath.lastIndexOf("/")), {
    recursive: true,
  });
  const source = [
    "---",
    'title: "Ugoite v0.1.2 — Safer everyday workflows"',
    "---",
    "",
    "This is the release-specific explanation.",
    "",
  ].join("\n");
  await Deno.writeTextFile(notePath, source);

  assertEquals(
    await validateReleaseNote({ version: "0.1.2", repoRoot }),
    source,
  );
  assertEquals(
    await validateReleaseNote({ version: "0.1.2", sourcePath: notePath }),
    source,
  );
});

Deno.test("REQ-OPS-026: the current repository version has a valid manual note", async () => {
  const version = (await Deno.readTextFile("version.txt")).trim();
  const source = await validateReleaseNote({ version });
  assertEquals(source.includes(`title: "Ugoite v${version}`), true);
});

Deno.test("REQ-OPS-026: the validator rejects missing, empty, mismatched, and prerelease notes", async () => {
  const repoRoot = await Deno.makeTempDir({ prefix: "ugoite-release-notes-" });
  const notePath = releaseNotePath("0.1.2", repoRoot);
  await Deno.mkdir(notePath.substring(0, notePath.lastIndexOf("/")), {
    recursive: true,
  });

  await assertFails(
    () => validateReleaseNote({ version: "0.1.2", repoRoot }),
    "was not found",
  );
  await Deno.writeTextFile(notePath, "\n");
  await assertFails(
    () => validateReleaseNote({ version: "0.1.2", repoRoot }),
    "is empty",
  );
  await Deno.writeTextFile(notePath, "# Ugoite v0.1.1 — Older release\n");
  await assertFails(
    () => validateReleaseNote({ version: "0.1.2", repoRoot }),
    "does not match",
  );
  await assertFails(
    () => validateReleaseNote({ version: "0.1.2-beta.1", repoRoot }),
    "stable SemVer",
  );
});

Deno.test("REQ-OPS-026: the candidate and publish workflows use exact manual note bytes", async () => {
  const candidate = await Deno.readTextFile(
    ".github/workflows/release-candidate.yml",
  );
  const publish = await Deno.readTextFile(
    ".github/workflows/release-publish.yml",
  );
  const validator = await Deno.readTextFile("tools/release-notes.ts");

  assertEquals(candidate.includes("release:validate-notes"), true);
  assertEquals(validator.includes("docs/version/releases"), true);
  assertEquals(publish.includes("publish-release-notes:"), true);
  assertEquals(publish.includes("publish-channel-release-notes:"), false);
  assertEquals(publish.includes("RELEASE_SOURCE_SHA"), true);
  assertEquals(publish.includes('git show "${RELEASE_SOURCE_SHA}:'), true);
  assertEquals(publish.includes("release:validate-notes"), true);
  assertEquals(
    publish.includes('gh release edit "${RELEASE_TAG}" --notes-file'),
    true,
  );
  assertEquals(publish.includes("UGOITE-CHANNEL-NOTES"), false);

  const distributionIndex = publish.indexOf("verify-distribution:");
  const notesJobIndex = publish.indexOf("publish-release-notes:");
  const aliasesIndex = publish.indexOf("promote-aliases:");
  assertEquals(notesJobIndex > distributionIndex, true);
  assertEquals(aliasesIndex > notesJobIndex, true);
  assertEquals(publish.includes("publish-release-notes"), true);
});

Deno.test("REQ-OPS-026: release documentation does not describe channel rendering as active", async () => {
  const changelog = await Deno.readTextFile(
    "docs/architecture/release/changelog.md",
  );
  const stable = await Deno.readTextFile(
    "docs/architecture/release/changelog-stable.md",
  );
  assertEquals(changelog.includes("rendered into a marked section"), false);
  assertEquals(changelog.includes("manual note"), true);
  assertEquals(stable.includes("docs/version/releases/v<version>.md"), true);
  assertEquals(stable.includes("Historical channel metadata"), true);
  assertEquals(
    stable.includes("is rendered into the GitHub Release body"),
    false,
  );
});

async function assertFails(
  operation: () => Promise<unknown>,
  message: string,
): Promise<void> {
  try {
    await operation();
  } catch (error) {
    assertEquals(String(error).includes(message), true);
    return;
  }
  throw new Error(`expected operation to fail with ${message}`);
}
