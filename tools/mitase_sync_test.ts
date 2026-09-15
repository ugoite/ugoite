import { assert } from "@std/assert/assert";
import { assertEquals } from "@std/assert/equals";
import {
  archiveName,
  parseSyncArgs,
  releaseArchiveUrl,
  renderLock,
  sha256Hex,
  type SyncRequest,
  validateSyncVersion,
  verifyArchive,
} from "./mitase_sync.ts";

const DIGEST_A = "0".repeat(64);
const DIGEST_B = "1".repeat(64);
const DIGEST_C = "2".repeat(64);
const DIGEST_D = "3".repeat(64);

function validArgs(version = "0.1.4"): string[] {
  return [
    "--version",
    version,
    "--sha256",
    `x86_64-unknown-linux-gnu=${DIGEST_A}`,
    "--sha256=aarch64-unknown-linux-gnu=" + DIGEST_B,
    "--sha256=x86_64-apple-darwin=" + DIGEST_C,
    "--sha256=aarch64-apple-darwin=" + DIGEST_D,
  ];
}

function request(version = "0.1.4"): SyncRequest {
  return parseSyncArgs(validArgs(version), {}, "/tmp/mitase.lock.toml");
}

Deno.test("mitase sync accepts an exact 0.1.x version with four digests", () => {
  const parsed = request("0.1.9");
  assertEquals(parsed.version, "0.1.9");
  assertEquals(parsed.digests.size, 4);
  assertEquals(
    parsed.digests.get("x86_64-unknown-linux-gnu"),
    DIGEST_A,
  );
  assertEquals(
    releaseArchiveUrl(parsed.baseUrl, parsed.version, "aarch64-apple-darwin"),
    "https://github.com/ugoite/mitase/releases/download/v0.1.9/mitase-v0.1.9-aarch64-apple-darwin.tar.gz",
  );
  assertEquals(
    archiveName("0.1.9", "x86_64-apple-darwin"),
    "mitase-v0.1.9-x86_64-apple-darwin.tar.gz",
  );
});

Deno.test("mitase sync rejects non-0.1 versions and mutable inputs", () => {
  for (const bad of ["0.2.0", "1.0.0", "0.1", "latest", "main", "", "v0.1.4"]) {
    let threw = false;
    try {
      validateSyncVersion(bad);
    } catch {
      threw = true;
    }
    assert(threw, `version must be rejected: ${JSON.stringify(bad)}`);
  }
  validateSyncVersion("0.1.0");
});

Deno.test("mitase sync requires every target digest and rejects extras", () => {
  const cases: string[][] = [
    ["--version", "0.1.4"],
    [...validArgs().slice(0, 3)],
    [...validArgs(), "--sha256", "riscv64-unknown-linux-gnu=" + DIGEST_A],
    [...validArgs(), "--sha256", "x86_64-unknown-linux-gnu=xyz"],
    [...validArgs(), "--bogus", "x"],
  ];
  for (const args of cases) {
    let threw = false;
    try {
      parseSyncArgs(args, {}, "/tmp/mitase.lock.toml");
    } catch {
      threw = true;
    }
    assert(threw, `args must be rejected: ${JSON.stringify(args)}`);
  }
  let latestBase = false;
  try {
    parseSyncArgs(validArgs(), {
      MITASE_RELEASE_BASE_URL: "https://example.invalid/mitase/latest",
    }, "/tmp/mitase.lock.toml");
  } catch {
    latestBase = true;
  }
  assert(latestBase, "mutable latest base URL must be rejected");
});

Deno.test("mitase lock rendering is deterministic", () => {
  const first = renderLock(request());
  const second = renderLock(request());
  assertEquals(first, second);
  assertEquals(
    first,
    `version = "0.1.4"

[target.x86_64-unknown-linux-gnu]
sha256 = "${DIGEST_A}"

[target.aarch64-unknown-linux-gnu]
sha256 = "${DIGEST_B}"

[target.x86_64-apple-darwin]
sha256 = "${DIGEST_C}"

[target.aarch64-apple-darwin]
sha256 = "${DIGEST_D}"
`,
  );
});

async function fixtureArchive(
  entries: Record<string, string>,
): Promise<Uint8Array> {
  const dir = await Deno.makeTempDir({ prefix: "ugoite-mitase-fixture." });
  try {
    for (const [name, contents] of Object.entries(entries)) {
      const path = `${dir}/${name}`;
      await Deno.writeTextFile(path, contents);
      await Deno.chmod(path, 0o755);
    }
    const names = Object.keys(entries);
    const process = new Deno.Command("tar", {
      args: ["-czf", `${dir}/fixture.tar.gz`, "-C", dir, ...names],
    });
    const output = await process.output();
    assert(output.success, "fixture archive must build");
    return await Deno.readFile(`${dir}/fixture.tar.gz`);
  } finally {
    await Deno.remove(dir, { recursive: true }).catch(() => {});
  }
}

Deno.test("mitase sync verifies shape and digest before writing anything", async () => {
  const req = request();
  const workDir = await Deno.makeTempDir({ prefix: "ugoite-mitase-verify." });
  try {
    const good = await fixtureArchive({
      mitase: "#!/bin/sh\necho 'mitase 0.1.4'\n",
    });
    // Wrong digest fails with the repository left unchanged.
    let mismatch = false;
    try {
      await verifyArchive(good, req, "x86_64-unknown-linux-gnu", workDir);
    } catch (error) {
      mismatch = String(error).includes("SHA-256 mismatch");
    }
    assert(mismatch, "digest mismatch must fail closed");
    // A multi-entry archive fails even with a matching digest.
    const multi = await fixtureArchive({
      mitase: "#!/bin/sh\n",
      extra: "unexpected\n",
    });
    const multiDigest = await sha256Hex(multi);
    const multiReq = parseSyncArgs(
      validArgs().map((arg) =>
        arg.startsWith("x86_64-unknown-linux-gnu=")
          ? `x86_64-unknown-linux-gnu=${multiDigest}`
          : arg
      ),
      {},
      "/tmp/mitase.lock.toml",
    );
    let shape = false;
    try {
      await verifyArchive(multi, multiReq, "x86_64-unknown-linux-gnu", workDir);
    } catch (error) {
      shape = String(error).includes("exactly one mitase entry");
    }
    assert(shape, "multi-entry archives must fail closed");
    // A well-formed single-entry archive verifies and stays executable.
    const goodDigest = await sha256Hex(good);
    const goodReq = parseSyncArgs(
      validArgs().map((arg) =>
        arg.startsWith("x86_64-unknown-linux-gnu=")
          ? `x86_64-unknown-linux-gnu=${goodDigest}`
          : arg
      ),
      {},
      "/tmp/mitase.lock.toml",
    );
    const extracted = await verifyArchive(
      good,
      goodReq,
      "x86_64-unknown-linux-gnu",
      workDir,
    );
    const stat = await Deno.stat(`${extracted}/mitase`);
    assert(
      stat.mode !== null && (stat.mode & 0o111) !== 0,
      "extracted mitase stays executable",
    );
  } finally {
    await Deno.remove(workDir, { recursive: true }).catch(() => {});
  }
});
