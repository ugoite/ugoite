import type { Asset } from "./types";
import { apiFetch } from "./api";

/** Asset API client */
export const assetApi = {
	/** Upload an asset */
	async upload(spaceId: string, file: File | Blob, filename?: string): Promise<Asset> {
		const formData = new FormData();
		formData.append("file", file, filename);
		const res = await apiFetch(`/spaces/${spaceId}/assets`, {
			method: "POST",
			body: formData,
		});
		if (!res.ok) {
			throw new Error(`Failed to upload asset: ${res.statusText}`);
		}
		return (await res.json()) as Asset;
	},

	/** List all assets in space */
	async list(spaceId: string): Promise<Asset[]> {
		const res = await apiFetch(`/spaces/${spaceId}/assets`);
		if (!res.ok) {
			throw new Error(`Failed to list assets: ${res.statusText}`);
		}
		return (await res.json()) as Asset[];
	},

	/** Delete an asset (fails if referenced) */
	async delete(spaceId: string, assetId: string): Promise<{ status: string; id: string }> {
		const res = await apiFetch(`/spaces/${spaceId}/assets/${assetId}`, {
			method: "DELETE",
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to delete asset: ${res.statusText}`);
		}
		return (await res.json()) as { status: string; id: string };
	},
};
