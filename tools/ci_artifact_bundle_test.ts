import { assertEquals } from "@std/assert/equals";
import { assertRejects } from "@std/assert/rejects";
import { assertThrows } from "@std/assert/throws";
import {
  parseArtifactSelection,
  validateCliArchiveMembers,
  verifyE2eBundle,
} from "./ci_artifact_bundle.ts";

const sourceSha = "a".repeat(40);
const runId = "123456789";

type BundleFixtureOptions = {
  kinds?: Array<"cli" | "image">;
  paths?: { cli: string; image: string };
  schemaVersion?: number;
};

async function writeBundle(
  root: string,
  {
    kinds = ["cli", "image"],
    paths = {
      cli: "cli/ugoite-v0.2.0-linux.tar.gz",
      image: "image/ugoite-e2e-docker.tar.gz",
    },
    schemaVersion = 2,
  }: BundleFixtureOptions = {},
): Promise<void> {
  const artifactFiles = [];
  for (const kind of kinds) {
    const relative = paths[kind];
    const full = `${root}/${relative}`;
    await Deno.mkdir(full.slice(0, full.lastIndexOf("/")), { recursive: true });
    await Deno.writeTextFile(full, relative);
    const bytes = await Deno.readFile(full);
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    artifactFiles.push({
      kind,
      file: {
        path: relative,
        sha256: [...new Uint8Array(digest)].map((byte) =>
          byte.toString(16).padStart(2, "0")
        ).join(""),
        size: bytes.byteLength,
      },
    });
  }
  const manifest = {
    schema_version: schemaVersion,
    source_sha: sourceSha,
    ci_run_id: runId,
    artifacts: artifactFiles.map(({ kind, file }) => ({
      kind,
      files: [file],
    })),
  };
  await Deno.writeTextFile(
    `${root}/manifest.json`,
    JSON.stringify(manifest),
  );
}

Deno.test("CI E2E artifact bundle verifies source, run identity, and file bytes", async () => {
  const root = await Deno.makeTempDir({ prefix: "ugoite-ci-artifacts-" });
  try {
    await writeBundle(root);
    const bundle = await verifyE2eBundle(root, sourceSha, runId);
    assertEquals(bundle.cliArchivePath?.endsWith(".tar.gz"), true);
    assertEquals(bundle.imageArchivePath?.endsWith(".tar.gz"), true);
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("CI artifact loader verifies only the requested artifact selection", async () => {
  const runtimeRoot = await Deno.makeTempDir({ prefix: "ugoite-ci-runtime-" });
  const cliRoot = await Deno.makeTempDir({ prefix: "ugoite-ci-cli-" });
  try {
    await writeBundle(runtimeRoot, { kinds: ["image"] });
    const runtime = await verifyE2eBundle(
      runtimeRoot,
      sourceSha,
      runId,
      sourceSha,
      "runtime",
    );
    assertEquals(runtime.cliArchivePath, null);
    assertEquals(runtime.imageArchivePath?.endsWith(".tar.gz"), true);
    await assertRejects(
      () => verifyE2eBundle(runtimeRoot, sourceSha, runId),
      Error,
      "exactly one cli entry",
    );

    await writeBundle(cliRoot, { kinds: ["cli"] });
    const cli = await verifyE2eBundle(
      cliRoot,
      sourceSha,
      runId,
      sourceSha,
      "cli",
    );
    assertEquals(cli.cliArchivePath?.endsWith(".tar.gz"), true);
    assertEquals(cli.imageArchivePath, null);
    await assertRejects(
      () => verifyE2eBundle(cliRoot, sourceSha, runId),
      Error,
      "exactly one image entry",
    );
  } finally {
    await Deno.remove(runtimeRoot, { recursive: true });
    await Deno.remove(cliRoot, { recursive: true });
  }
});

Deno.test("CI artifact loader rejects unsupported selections and manifest schemas", async () => {
  assertEquals(parseArtifactSelection(undefined), "both");
  assertEquals(parseArtifactSelection(" runtime "), "runtime");
  assertEquals(parseArtifactSelection("cli"), "cli");
  assertThrows(() => parseArtifactSelection("server"), Error, "unknown");

  const root = await Deno.makeTempDir({ prefix: "ugoite-ci-schema-" });
  try {
    await writeBundle(root, { schemaVersion: 99 });
    await assertRejects(
      () => verifyE2eBundle(root, sourceSha, runId),
      Error,
      "unsupported artifact manifest schema",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("CI E2E artifact bundle rejects mismatched identities and changed bytes", async () => {
  const root = await Deno.makeTempDir({ prefix: "ugoite-ci-artifacts-" });
  try {
    await writeBundle(root);
    await assertRejects(
      () => verifyE2eBundle(root, "b".repeat(40), runId),
      Error,
      "source SHA",
    );
    await assertRejects(
      () => verifyE2eBundle(root, sourceSha, runId, "b".repeat(40)),
      Error,
      "checkout",
    );
    await assertRejects(
      () => verifyE2eBundle(root, sourceSha, "987654321"),
      Error,
      "run ID",
    );
    await Deno.writeTextFile(
      `${root}/image/ugoite-e2e-docker.tar.gz`,
      "changed",
    );
    await assertRejects(
      () => verifyE2eBundle(root, sourceSha, runId),
      Error,
      "checksum",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("CI E2E artifact bundle rejects a changed manifest size", async () => {
  const root = await Deno.makeTempDir({ prefix: "ugoite-ci-artifact-size-" });
  try {
    await writeBundle(root);
    const manifest = JSON.parse(
      await Deno.readTextFile(`${root}/manifest.json`),
    );
    const image = manifest.artifacts.find((artifact: { kind: string }) =>
      artifact.kind === "image"
    );
    image.files[0].size += 1;
    await Deno.writeTextFile(`${root}/manifest.json`, JSON.stringify(manifest));
    await assertRejects(
      () => verifyE2eBundle(root, sourceSha, runId),
      Error,
      "size mismatch",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("CI E2E artifact bundle rejects a symlink at a manifest path", async () => {
  const root = await Deno.makeTempDir({
    prefix: "ugoite-ci-artifact-symlink-",
  });
  try {
    await writeBundle(root);
    const actualPath = `${root}/image/ugoite-e2e-docker.tar.gz`;
    const linkPath = `${root}/image/alias.tar.gz`;
    await Deno.symlink("ugoite-e2e-docker.tar.gz", linkPath);
    const manifest = JSON.parse(
      await Deno.readTextFile(`${root}/manifest.json`),
    );
    const image = manifest.artifacts.find((artifact: { kind: string }) =>
      artifact.kind === "image"
    );
    image.files[0].path = "image/alias.tar.gz";
    await Deno.writeTextFile(`${root}/manifest.json`, JSON.stringify(manifest));
    assertEquals((await Deno.readFile(actualPath)).byteLength > 0, true);
    await assertRejects(
      () => verifyE2eBundle(root, sourceSha, runId),
      Error,
      "contains a symlink",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("CI E2E artifact bundle rejects paths outside its root", async () => {
  const root = await Deno.makeTempDir({ prefix: "ugoite-ci-artifacts-" });
  try {
    await writeBundle(root);
    const manifest = JSON.parse(
      await Deno.readTextFile(`${root}/manifest.json`),
    );
    manifest.artifacts[0].files[0].path = "../outside.tar.gz";
    await Deno.writeTextFile(
      `${root}/manifest.json`,
      JSON.stringify(manifest),
    );
    await assertRejects(
      () => verifyE2eBundle(root, sourceSha, runId),
      Error,
      "unsafe artifact manifest path",
    );
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("CI CLI archive allowlist rejects traversal, extra entries, and links", () => {
  validateCliArchiveMembers(
    "ugoite\n",
    "-rwxr-xr-x user group 1234 2026-09-28 00:00 ugoite\n",
  );
  assertThrows(
    () =>
      validateCliArchiveMembers(
        "../ugoite\n",
        "-rwxr-xr-x user group 1234 2026-09-28 00:00 ../ugoite\n",
      ),
    Error,
    "regular ugoite binary",
  );
  assertThrows(
    () =>
      validateCliArchiveMembers(
        "ugoite\n",
        "lrwxrwxrwx user group 7 2026-09-28 00:00 ugoite -> /etc/passwd\n",
      ),
    Error,
    "regular ugoite binary",
  );
  assertThrows(
    () =>
      validateCliArchiveMembers(
        "ugoite\nextra\n",
        "-rwxr-xr-x user group 1234 2026-09-28 00:00 ugoite\n",
      ),
    Error,
    "regular ugoite binary",
  );
});
