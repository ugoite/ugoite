import type { AssetReference } from "./types";
import { protocolFetch, protocolFetchResponse } from "./ugoite-client/protocol";

const ASSET_UPLOAD_OPERATION = "asset.upload";
const ASSET_DELETE_OPERATION = "asset.delete";
const ASSET_READ_OPERATION = "asset.read";

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
      ASSET_UPLOAD_OPERATION,
      { space_id: spaceId },
      undefined,
      { body: formData, signal },
    );
  },

  async delete(
    spaceId: string,
    assetId: string,
  ): Promise<{ status: string; id: string }> {
    return await protocolFetch<{ status: string; id: string }>(ASSET_DELETE_OPERATION, {
      space_id: spaceId,
      asset_id: assetId,
    });
  },

  async read(
    spaceId: string,
    assetId: string,
    formName: string,
    entryId: string,
    signal?: AbortSignal,
  ): Promise<Blob> {
    const response = await protocolFetchResponse(ASSET_READ_OPERATION, {
      space_id: spaceId,
      asset_id: assetId,
      form: formName,
      entry_id: entryId,
    }, { signal });
    return await response.blob();
  },
};
