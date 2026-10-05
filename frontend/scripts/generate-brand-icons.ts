import sharp from "npm:sharp@0.34.5";
import { createHash } from "node:crypto";
import { dirname, relative } from "node:path";

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

// The social preview wordmark is fixed vector geometry, not host-resolved
// text. A previous revision rendered "Ugoite" through an SVG text element
// with a system sans-serif font, so the PNG rasterized differently depending
// on the fonts installed on the machine running the generator. These paths,
// circles, and strokes use absolute coordinates only, which keeps the output
// byte-identical on every host with the pinned renderer below. The geometry
// shares the previous label's baseline (y=346), cap top (y=252), and ink
// (#111111); only the rendering method changed.
const socialWordmark =
  `<g fill="none" stroke="#111111" stroke-width="24" stroke-linecap="round" stroke-linejoin="round"><path d="M607,264 V308 A26,26 0 0 0 659,308 V264"/><circle cx="719" cy="312" r="22"/><path d="M741,282 V338 C741,360 729,370 711,366"/><circle cx="811" cy="312" r="22"/><path d="M879,290 V334"/><circle cx="879" cy="258" r="12" fill="#111111" stroke="none"/><path d="M941,262 V318 Q941,342 965,338"/><path d="M917,296 H965"/><path d="M1043,300 A27,27 0 1 0 1043,324"/><path d="M1001,312 H1041"/></g>`;

const socialSvg =
  `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640"><rect width="1280" height="640" fill="#FFFFFF"/>${socialWordmark}</svg>`;

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
