import { assert, assertEquals } from "@std/assert";
import sharp from "sharp";
import { createHash } from "node:crypto";
import { getBrandIconPrecacheEntries } from "../frontend/src/lib/brand-icon-precache.ts";

const manifest = JSON.parse(
  await Deno.readTextFile("docs/brand/assets/manifest.json"),
) as {
  source: string;
  sourceSha256: string;
  renderer: string;
  outputs: Array<{
    path: string;
    width: number;
    height: number;
    sha256: string;
  }>;
};
const hash = (bytes: Uint8Array) =>
  createHash("sha256").update(bytes).digest("hex");

Deno.test("brand icon provenance matches all generated files", async () => {
  assertEquals(manifest.source, "frontend/public/brand/ugoite-mark.svg");
  assertEquals(manifest.renderer, "sharp@0.34.5");
  assertEquals(
    hash(await Deno.readFile(manifest.source)),
    manifest.sourceSha256,
  );
  assertEquals(
    manifest.outputs.map(({ path }) => path),
    [
      "frontend/public/brand/ugoite-icon-square.svg",
      "frontend/public/favicon.ico",
      "frontend/public/apple-touch-icon.png",
      "frontend/public/icons/ugoite-192.png",
      "frontend/public/icons/ugoite-512.png",
      "docs/brand/assets/ugoite-github-avatar-512.png",
      "docs/brand/assets/ugoite-github-social-1280x640.png",
    ],
  );
  assertEquals(
    getBrandIconPrecacheEntries(manifest.outputs).map(({ url }) => url),
    [
      "/brand/ugoite-icon-square.svg",
      "/favicon.ico",
      "/apple-touch-icon.png",
      "/icons/ugoite-192.png",
      "/icons/ugoite-512.png",
    ],
  );

  for (const output of manifest.outputs) {
    const bytes = await Deno.readFile(output.path);
    assertEquals(hash(bytes), output.sha256, `${output.path} SHA-256`);
    if (output.path.endsWith(".png")) {
      const metadata = await sharp(bytes).metadata();
      assertEquals(metadata.width, output.width, `${output.path} width`);
      assertEquals(metadata.height, output.height, `${output.path} height`);
    }
  }

  const social = manifest.outputs.find((entry) =>
    entry.path.endsWith("ugoite-github-social-1280x640.png")
  );
  assert(social);
  assert((await Deno.stat(social.path)).size < 1_000_000);
});

Deno.test("favicon is an ICO with 16, 32, 48, and 64 pixel PNG frames", async () => {
  const bytes = await Deno.readFile("frontend/public/favicon.ico");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  assertEquals(view.getUint16(0, true), 0);
  assertEquals(view.getUint16(2, true), 1);
  const frameCount = view.getUint16(4, true);
  assertEquals(frameCount, 4);
  const dimensions: number[] = [];
  for (let index = 0; index < frameCount; index++) {
    const offset = 6 + index * 16;
    dimensions.push(bytes[offset] || 256);
    const length = view.getUint32(offset + 8, true);
    const imageOffset = view.getUint32(offset + 12, true);
    assertEquals(bytes[imageOffset], 0x89, "PNG signature");
    assert(length > 0);
    assert(imageOffset + length <= bytes.length);
  }
  assertEquals(dimensions, [16, 32, 48, 64]);
});
