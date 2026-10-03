const RELEASE_TAG = "v0.2.1";
const RELEASE_SOURCE_SHA = "51f469e3f81113e4295b8d579410c538fd36a61c";
const RELEASE_MANIFEST_SHA256 =
  "2d9a9eefd050bab9c6b90579a82a397370ec722da1a373fb26aaad2e7feb6207";

type ReleaseAsset = {
  archive: string;
  sha256: string;
};

const assets: Record<string, ReleaseAsset> = {
  "darwin-aarch64": {
    archive: "ugoite-v0.2.1-aarch64-apple-darwin.tar.gz",
    sha256: "2e0be50101b40f4406a158e70f1321e00d78919d0a2ddce54c224428aac5ded2",
  },
  "darwin-x86_64": {
    archive: "ugoite-v0.2.1-x86_64-apple-darwin.tar.gz",
    sha256: "82bcfbb6b17935b294ea4cef89219a1f94d0b871cbbddd207f014fd6ae699b53",
  },
  "linux-aarch64": {
    archive: "ugoite-v0.2.1-aarch64-unknown-linux-gnu.tar.gz",
    sha256: "156eb977e2dacfacc4a5b05dfb38f177c19e7e565ca4a15b7b155ad85b8518d5",
  },
  "linux-x86_64": {
    archive: "ugoite-v0.2.1-x86_64-unknown-linux-gnu.tar.gz",
    sha256: "f597143fe39f6a07bb50b7cb8eafeca0db4cc85e07a42d8a549ff4d5c7d5b503",
  },
};

function sha256(bytes: Uint8Array): Promise<ArrayBuffer> {
  const owned = bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  ) as ArrayBuffer;
  return crypto.subtle.digest("SHA-256", owned);
}

function hex(bytes: ArrayBuffer): string {
  return Array.from(
    new Uint8Array(bytes),
    (value) => value.toString(16).padStart(2, "0"),
  ).join("");
}

async function fetchVerified(
  url: string,
  expectedSha256: string,
  label: string,
): Promise<Uint8Array> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${label} download failed: ${response.status}`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  const actualSha256 = hex(await sha256(bytes));
  if (actualSha256 !== expectedSha256) {
    throw new Error(
      `${label} digest mismatch: expected ${expectedSha256}, got ${actualSha256}`,
    );
  }
  return bytes;
}

async function findCli(directory: string): Promise<string | undefined> {
  for await (const entry of Deno.readDir(directory)) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory) {
      const nested = await findCli(path);
      if (nested) return nested;
    } else if (entry.isFile && entry.name === "ugoite") {
      return path;
    }
  }
  return undefined;
}

const assetKey = `${Deno.build.os}-${Deno.build.arch}`;
const asset = assets[assetKey];
if (!asset) {
  throw new Error(`v0.2.1 downgrade check does not support ${assetKey}`);
}

const releaseBase =
  `https://github.com/ugoite/ugoite/releases/download/${RELEASE_TAG}`;
const manifestBytes = await fetchVerified(
  `${releaseBase}/release-manifest.json`,
  RELEASE_MANIFEST_SHA256,
  `${RELEASE_TAG} release manifest`,
);
const manifest = JSON.parse(new TextDecoder().decode(manifestBytes)) as {
  release_tag?: string;
  version?: string;
  source_sha?: string;
  files?: Array<{ name: string; sha256: string }>;
};
if (
  manifest.release_tag !== RELEASE_TAG || manifest.version !== "0.2.1" ||
  manifest.source_sha !== RELEASE_SOURCE_SHA
) {
  throw new Error(
    `${RELEASE_TAG} release manifest does not match the pinned source`,
  );
}
const manifestAsset = manifest.files?.find((file) =>
  file.name === asset.archive
);
if (!manifestAsset || manifestAsset.sha256 !== asset.sha256) {
  throw new Error(
    `${RELEASE_TAG} manifest does not match the pinned CLI archive`,
  );
}

const archiveBytes = await fetchVerified(
  `${releaseBase}/${asset.archive}`,
  asset.sha256,
  `${RELEASE_TAG} CLI archive`,
);
const temporaryRoot = await Deno.makeTempDir({
  prefix: "ugoite-composition-downgrade-",
});
try {
  const archivePath = `${temporaryRoot}/v0.2.1-cli.tar.gz`;
  await Deno.writeFile(archivePath, archiveBytes);
  const extract = await new Deno.Command("tar", {
    args: ["-xzf", archivePath, "-C", temporaryRoot],
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (extract.code !== 0) {
    throw new Error(
      `extract v0.2.1 CLI failed: ${new TextDecoder().decode(extract.stderr)}`,
    );
  }
  const cli = await findCli(temporaryRoot);
  if (!cli) throw new Error("v0.2.1 archive does not contain the ugoite CLI");

  const test = await new Deno.Command("cargo", {
    args: [
      "test",
      "--locked",
      "-p",
      "ugoite-iceberg",
      "--test",
      "test_composition_downgrade",
      "--",
      "v021_reader_writer_preserves_composition_and_unrelated_knowledge",
      "--exact",
      "--ignored",
    ],
    env: { UGOITE_V021_CLI_PATH: cli },
    stdout: "inherit",
    stderr: "inherit",
  }).output();
  if (test.code !== 0) {
    throw new Error(
      `Composition downgrade integration test failed with ${test.code}`,
    );
  }
  console.log(
    `Composition downgrade verified with ${RELEASE_TAG} CLI from ${RELEASE_SOURCE_SHA}`,
  );
} finally {
  await Deno.remove(temporaryRoot, { recursive: true });
}
