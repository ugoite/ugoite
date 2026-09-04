import { assertEquals } from "@std/assert/equals";
import {
  channelForVersion,
  composeReleaseNotes,
  renderReleaseNotes,
} from "./release-notes.ts";

Deno.test("REQ-OPS-026: release versions select the matching note channel", () => {
  assertEquals(channelForVersion("0.1.0"), "stable");
  assertEquals(channelForVersion("0.1.0-beta.2"), "beta");
  assertEquals(channelForVersion("0.1.0-alpha.7"), "alpha");
});

Deno.test("REQ-OPS-026: all channel sources render versioned release notes", async () => {
  for (
    const [channel, version] of [
      ["stable", "0.1.0"],
      ["beta", "0.1.0-beta.2"],
      ["alpha", "0.1.0-alpha.7"],
    ] as const
  ) {
    const rendered = await renderReleaseNotes({ channel, version });
    assertEquals(rendered.includes(`# v${version}`), true);
    assertEquals(
      rendered.includes(`docs/version/changelog/${channel}.yaml`),
      true,
    );
    assertEquals(rendered.includes("## Expectations"), true);
    assertEquals(rendered.includes("## Planned"), true);
  }
});

Deno.test("REQ-OPS-026: invalid channel sources fail before composition", async () => {
  const repoRoot = await Deno.makeTempDir({ prefix: "ugoite-release-notes-" });
  const sourcePath = `${repoRoot}/docs/version/changelog/stable.yaml`;
  const docPath = `${repoRoot}/docs/architecture/release/changelog-stable.md`;
  await Deno.mkdir(`${repoRoot}/docs/version/changelog`, { recursive: true });
  await Deno.mkdir(`${repoRoot}/docs/architecture/release`, {
    recursive: true,
  });
  await Deno.writeTextFile(docPath, "---\ntitle: Stable\n---\n");
  await Deno.writeTextFile(
    sourcePath,
    [
      "channel: beta",
      "title: Stable",
      "doc_path: docs/architecture/release/changelog-stable.md",
      "summary: Summary",
      "release_notes:",
      "  intro: Intro",
      "  expectations:",
      "  - Expectation",
      "  added:",
      "  - Added",
      "  changed:",
      "  - Changed",
      "  planned:",
      "  - Planned",
      "",
    ].join("\n"),
  );

  await assertFails(
    () =>
      renderReleaseNotes({
        channel: "stable",
        version: "0.1.0",
        repoRoot,
        sourcePath,
      }),
    "channel must be stable",
  );
  await assertFails(
    () =>
      renderReleaseNotes({
        channel: "beta",
        version: "0.1.0",
        repoRoot,
        sourcePath,
      }),
    "channel beta does not match release version",
  );
  await Deno.writeTextFile(
    sourcePath,
    [
      "channel: stable",
      "title: Stable",
      "doc_path: docs/architecture/release/changelog-stable.md",
      "summary: Summary",
      "release_notes:",
      "  intro: Intro",
      "  expectations:",
      "  - Expectation",
      "  added:",
      "  - Added",
      "  changed:",
      "  - Changed",
      "",
    ].join("\n"),
  );
  await assertFails(
    () =>
      renderReleaseNotes({
        channel: "stable",
        version: "0.1.0",
        repoRoot,
        sourcePath,
      }),
    "planned must be a non-empty list",
  );
});

Deno.test("REQ-OPS-026: marked channel notes replace once and preserve generated notes", () => {
  const existingBody = "## Generated changes\n\n- Keep this summary";
  const channelNotes =
    "# v0.1.0 Stable Channel Changelog\n\n## Added\n\n- One change";
  const first = composeReleaseNotes({
    channel: "stable",
    version: "0.1.0",
    existingBody,
    channelNotes,
  });
  assertEquals(first.includes(existingBody), true);
  assertEquals((first.match(/UGOITE-CHANNEL-NOTES:v1:start/g) ?? []).length, 1);
  assertEquals((first.match(/UGOITE-CHANNEL-NOTES:v1:end/g) ?? []).length, 1);

  const rerun = composeReleaseNotes({
    channel: "stable",
    version: "0.1.0",
    existingBody: first,
    channelNotes,
  });
  assertEquals(rerun, first);

  assertFailsSync(
    () =>
      composeReleaseNotes({
        channel: "stable",
        version: "0.1.0",
        existingBody: "<!-- UGOITE-CHANNEL-NOTES:v1:start -->",
        channelNotes: "# notes",
      }),
    "incomplete channel-notes marker",
  );
  assertFailsSync(
    () =>
      composeReleaseNotes({
        channel: "stable",
        version: "0.1.0",
        existingBody:
          "<!-- UGOITE-CHANNEL-NOTES:v1:start channel=beta version=0.1.0-beta.1 -->\nold\n<!-- UGOITE-CHANNEL-NOTES:v1:end -->",
        channelNotes: "# notes",
      }),
    "does not match the release channel or version",
  );
});

Deno.test("REQ-OPS-026: workflow updates notes after artifact quick-start verification", async () => {
  const workflow = await Deno.readTextFile(
    ".github/workflows/release-publish.yml",
  );
  const quickstartIndex = workflow.indexOf("verify-published-quickstarts:");
  const notesJobIndex = workflow.indexOf("publish-channel-release-notes:");
  assertEquals(notesJobIndex > quickstartIndex, true);
  assertEquals(workflow.includes("tools/release-notes.ts compose"), true);
  assertEquals(
    workflow.includes('gh release edit "${RELEASE_TAG}" --notes-file'),
    true,
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

function assertFailsSync(operation: () => unknown, message: string): void {
  try {
    operation();
  } catch (error) {
    assertEquals(String(error).includes(message), true);
    return;
  }
  throw new Error(`expected operation to fail with ${message}`);
}
