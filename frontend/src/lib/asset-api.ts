import type { AssetReference } from "./types";
import { protocolFetch, protocolFetchResponse } from "./ugoite-client/protocol";

/** Asset API client backed by the shared Rust/WASM protocol. */
export const assetApi = {
  async upload(
    spaceId: string,
    file: File | Blob,
    filename?: string,
    signal?: AbortSignal,
  ): Promise<AssetReference> {
    const formData = new FormData();
    formData.append("file", file, filename);
    return await protocolFetch<AssetReference>(
      "asset.upload",
      { space_id: spaceId },
      undefined,
      { body: formData, signal },
    );
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

  async read(
    spaceId: string,
    assetId: string,
    formName: string,
    entryId: string,
  ): Promise<Blob> {
    const response = await protocolFetchResponse("asset.read", {
      space_id: spaceId,
      asset_id: assetId,
      form: formName,
      entry_id: entryId,
    });
    return await response.blob();
  },
};
