import sharp from "npm:sharp@0.34.5";
import { createHash } from "node:crypto";
import { dirname, join, relative } from "node:path";

const root = new URL("../../", import.meta.url);
const frontend = new URL("../", import.meta.url);
const sourcePath = new URL("public/brand/ugoite-mark.svg", frontend);
const publicPath = new URL("public/", frontend);
const docsAssetsPath = new URL("../../docs/brand/assets/", import.meta.url);
const sourceBytes = await Deno.readFile(sourcePath);
const sourceSvg = new TextDecoder().decode(sourceBytes);
const innerMatch = sourceSvg.match(/<svg\b[^>]*>([\s\S]*?)<\/svg>/);
if (!innerMatch) throw new Error("Brand mark must be a complete SVG document");
const markContents = innerMatch[1].trim();

const sha256 = (bytes: Uint8Array) =>
  createHash("sha256").update(bytes).digest("hex");

function squareSvg(size: number, fillScale = 0.8): string {
  const height = size * fillScale;
  const width = height * 132 / 190;
  const x = (size - width) / 2;
  const y = (size - height) / 2;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${size} ${size}"><rect width="${size}" height="${size}" fill="#FFFFFF"/><svg x="${x}" y="${y}" width="${width}" height="${height}" viewBox="52 20 132 190">${markContents}</svg></svg>`;
}

async function png(svg: string, size: number): Promise<Uint8Array> {
  return await sharp(Buffer.from(svg)).resize(size, size).png().toBuffer();
}

function ico(frames: Uint8Array[]): Uint8Array {
  const headerSize = 6 + frames.length * 16;
  const header = Buffer.alloc(headerSize);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(frames.length, 4);
  let offset = headerSize;
  for (let index = 0; index < frames.length; index++) {
    const dimension = [16, 32, 48, 64][index];
    const entry = 6 + index * 16;
    header.writeUInt8(dimension === 256 ? 0 : dimension, entry);
    header.writeUInt8(dimension === 256 ? 0 : dimension, entry + 1);
    header.writeUInt8(0, entry + 2);
    header.writeUInt8(0, entry + 3);
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(frames[index].byteLength, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += frames[index].byteLength;
  }
  return Buffer.concat([header, ...frames]);
}

const socialSvg =
  `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640"><rect width="1280" height="640" fill="#FFFFFF"/><text x="595" y="346" font-family="Arial, Helvetica, sans-serif" font-size="132" font-weight="600" fill="#111111">Ugoite</text></svg>`;

const outputs: Array<
  { path: URL; bytes: Uint8Array; width: number; height: number }
> = [];
outputs.push({
  path: new URL("brand/ugoite-icon-square.svg", publicPath),
  bytes: new TextEncoder().encode(squareSvg(224)),
  width: 224,
  height: 224,
});
const icoFrames = await Promise.all(
  [16, 32, 48, 64].map((size) => png(squareSvg(size, 0.86), size)),
);
outputs.push({
  path: new URL("favicon.ico", publicPath),
  bytes: ico(icoFrames),
  width: 64,
  height: 64,
});
for (
  const [path, size] of [
    ["apple-touch-icon.png", 180],
    ["icons/ugoite-192.png", 192],
    ["icons/ugoite-512.png", 512],
  ] as const
) {
  outputs.push({
    path: new URL(path, publicPath),
    bytes: await png(squareSvg(size), size),
    width: size,
    height: size,
  });
}
outputs.push({
  path: new URL("ugoite-github-avatar-512.png", docsAssetsPath),
  bytes: await png(squareSvg(512), 512),
  width: 512,
  height: 512,
});
const social = await sharp(Buffer.from(socialSvg))
  .composite([{
    input: Buffer.from(await png(squareSvg(320), 320)),
    left: 120,
    top: 160,
  }])
  .png()
  .toBuffer();
outputs.push({
  path: new URL("ugoite-github-social-1280x640.png", docsAssetsPath),
  bytes: social,
  width: 1280,
  height: 640,
});

const manifest = {
  source: "frontend/public/brand/ugoite-mark.svg",
  sourceSha256: sha256(sourceBytes),
  generator: "frontend/scripts/generate-brand-icons.ts",
  renderer: "sharp@0.34.5",
  outputs: [] as Array<{
    path: string;
    width: number;
    height: number;
    sha256: string;
  }>,
};
for (const output of outputs) {
  await Deno.mkdir(dirname(output.path.pathname), { recursive: true });
  await Deno.writeFile(output.path, output.bytes);
  manifest.outputs.push({
    path: relative(
      new URL("../../", import.meta.url).pathname,
      output.path.pathname,
    ),
    width: output.width,
    height: output.height,
    sha256: sha256(output.bytes),
  });
}
await Deno.writeTextFile(
  new URL("manifest.json", docsAssetsPath),
  `${JSON.stringify(manifest, null, 2)}\n`,
);
console.log(JSON.stringify(manifest, null, 2));
