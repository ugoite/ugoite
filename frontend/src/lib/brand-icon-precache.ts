export type BrandIconOutput = {
  path: string;
  sha256: string;
};

export function getBrandIconPrecacheEntries(outputs: BrandIconOutput[]) {
  return outputs
    .filter((output) => output.path.startsWith("frontend/public/"))
    .map((output) => ({
      url: `/${output.path.slice("frontend/public/".length)}`,
      revision: output.sha256,
    }));
}
