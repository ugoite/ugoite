const [storageRoot, proofFile] = Deno.args;
if (!storageRoot || !proofFile) {
  throw new Error(
    "usage: deno run verify-portable-space-hashes.ts STORAGE_ROOT PROOF_FILE",
  );
}

type PortableProof = {
  space_uid: string;
  source_files: Record<string, string>;
  append_only_prefixes: Record<string, number[]>;
};

const proof = JSON.parse(await Deno.readTextFile(proofFile)) as PortableProof;
const mutablePaths = new Set([
  "_ugoite/catalog/head.json",
  "metadata/version-hint.text",
  "security/principals.json",
]);
let verified = 0;

for (const [relativePath, expectedHash] of Object.entries(proof.source_files)) {
  if (
    relativePath.startsWith("/") ||
    relativePath.split("/").includes("..") ||
    !/^[0-9a-f]{64}$/.test(expectedHash)
  ) {
    throw new Error(`invalid source file proof entry: ${relativePath}`);
  }
  if (
    mutablePaths.has(relativePath) ||
    relativePath.endsWith("/version-hint.text") ||
    relativePath.startsWith("_ugoite/derived/")
  ) continue;

  const path = `${storageRoot}/spaces/${proof.space_uid}/${relativePath}`;
  const bytes = await Deno.readFile(path);
  const expectedPrefix = proof.append_only_prefixes?.[relativePath];
  if (expectedPrefix) {
    const prefix = Uint8Array.from(expectedPrefix);
    if (
      bytes.length <= prefix.length ||
      !prefix.every((byte, index) => bytes[index] === byte)
    ) {
      throw new Error(
        `append-only authoritative file prefix changed or no claim audit was appended: ${relativePath}`,
      );
    }
    verified += 1;
    continue;
  }
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  const actualHash = [...digest].map((byte) =>
    byte.toString(16).padStart(2, "0")
  )
    .join("");
  if (actualHash !== expectedHash) {
    throw new Error(`copied authoritative file changed: ${relativePath}`);
  }
  verified += 1;
}

console.log(`Verified ${verified} copied authoritative file hashes.`);
