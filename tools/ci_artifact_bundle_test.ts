import { assertEquals } from "@std/assert/equals";
import { assertRejects } from "@std/assert/rejects";
import { verifyE2eBundle } from "./ci_artifact_bundle.ts";

const sourceSha = "a".repeat(40);
const runId = "123456789";

async function writeBundle(root: string, paths = {
  cli: "cli/ugoite-v0.2.0-linux.tar.gz",
  image: "image/ugoite-e2e-docker.tar.gz",
}): Promise<void> {
  for (const relative of Object.values(paths)) {
    const full = `${root}/${relative}`;
    await Deno.mkdir(full.slice(0, full.lastIndexOf("/")), { recursive: true });
    await Deno.writeTextFile(full, relative);
  }
  const file = async (path: string) => {
    const bytes = await Deno.readFile(`${root}/${path}`);
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    return {
      path,
      sha256: [...new Uint8Array(digest)].map((byte) =>
        byte.toString(16).padStart(2, "0")
      ).join(""),
      size: bytes.byteLength,
    };
  };
  const manifest = {
    schema_version: 2,
    source_sha: sourceSha,
    ci_run_id: runId,
    artifacts: [
      { kind: "cli", files: [await file(paths.cli)] },
      { kind: "image", files: [await file(paths.image)] },
    ],
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
    assertEquals(bundle.cliArchivePath.endsWith(".tar.gz"), true);
    assertEquals(bundle.imageArchivePath.endsWith(".tar.gz"), true);
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
