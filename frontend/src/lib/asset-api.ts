import type { Asset } from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

/** Asset API client backed by the shared Rust/WASM protocol. */
export const assetApi = {
  async upload(
    spaceId: string,
    file: File | Blob,
    filename?: string,
  ): Promise<Asset> {
    const formData = new FormData();
    formData.append("file", file, filename);
    return await protocolFetch<Asset>(
      "asset.upload",
      { space_id: spaceId },
      undefined,
      { body: formData },
    );
  },

  async list(spaceId: string): Promise<Asset[]> {
    return await protocolFetch<Asset[]>("asset.list", { space_id: spaceId });
  },

  async delete(
    spaceId: string,
    assetId: string,
  ): Promise<{ status: string; id: string }> {
    return await protocolFetch<{ status: string; id: string }>("asset.delete", {
      space_id: spaceId,
      asset_id: assetId,
    });
  },
};
