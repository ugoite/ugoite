import { createHash } from "node:crypto";

export async function containedArtifactPath(
  root: string,
  relativePath: string,
): Promise<string> {
  if (
    typeof relativePath !== "string" || relativePath.length === 0 ||
    relativePath.startsWith("/") || relativePath.includes("\\") ||
    relativePath.split("/").some((part) =>
      part === "" || part === "." || part === ".."
    )
  ) {
    throw new Error(`unsafe artifact manifest path: ${relativePath}`);
  }
  const rootPath = await Deno.realPath(root);
  let current = rootPath;
  const parts = relativePath.split("/");
  for (const [index, part] of parts.entries()) {
    current = `${current}/${part}`;
    const info = await Deno.lstat(current);
    if (info.isSymlink) {
      throw new Error(
        `artifact manifest path contains a symlink: ${relativePath}`,
      );
    }
    if (index < parts.length - 1 && !info.isDirectory) {
      throw new Error(
        `artifact manifest path crosses a non-directory: ${relativePath}`,
      );
    }
    if (index === parts.length - 1 && !info.isFile) {
      throw new Error(`artifact manifest path is not a file: ${relativePath}`);
    }
  }
  const resolved = await Deno.realPath(current);
  if (!resolved.startsWith(`${rootPath}/`)) {
    throw new Error(`artifact path escapes bundle root: ${relativePath}`);
  }
  return resolved;
}

export async function sha256File(path: string): Promise<{
  sha256: string;
  size: number;
}> {
  const file = await Deno.open(path, { read: true });
  const hash = createHash("sha256");
  let size = 0;
  for await (const chunk of file.readable) {
    hash.update(chunk);
    size += chunk.byteLength;
  }
  return { sha256: hash.digest("hex"), size };
}
