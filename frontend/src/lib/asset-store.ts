import { createSignal } from "solid-js";
import type { Asset } from "./types";
import { assetApi } from "./asset-api";

export function createAssetStore(spaceId: () => string) {
	const [assets, setAssets] = createSignal<Asset[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function loadAssets(): Promise<void> {
		setLoading(true);
		setError(null);
		try {
			const data = await assetApi.list(spaceId());
			setAssets(data);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load assets");
		} finally {
			setLoading(false);
		}
	}

	async function uploadAsset(file: File | Blob, filename?: string): Promise<Asset> {
		setError(null);
		const uploaded = await assetApi.upload(spaceId(), file, filename);
		await loadAssets();
		return uploaded;
	}

	async function deleteAsset(assetId: string): Promise<void> {
		setError(null);
		await assetApi.delete(spaceId(), assetId);
		await loadAssets();
	}

	return {
		assets,
		loading,
		error,
		loadAssets,
		uploadAsset,
		deleteAsset,
	};
}

export type AssetStore = ReturnType<typeof createAssetStore>;
