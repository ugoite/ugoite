import { A, useParams } from "@solidjs/router";
import { createResource, createSignal, Show } from "solid-js";
import { AssetUploader } from "~/components/AssetUploader";
import { assetApi } from "~/lib/asset-api";
import type { Asset } from "~/lib/types";

export default function SpaceAssetsRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;
	const [actionError, setActionError] = createSignal<string | null>(null);

	const [assets, { refetch }] = createResource(async () => {
		return await assetApi.list(spaceId());
	});

	const handleUpload = async (file: File): Promise<Asset> => {
		setActionError(null);
		const created = await assetApi.upload(spaceId(), file, file.name);
		await refetch();
		return created;
	};

	const handleRemove = async (assetId: string) => {
		setActionError(null);
		try {
			await assetApi.delete(spaceId(), assetId);
			await refetch();
		} catch (err) {
			setActionError(err instanceof Error ? err.message : "Failed to delete asset");
		}
	};

	return (
		<main class="ui-shell ui-page">
			<div class="max-w-4xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="ui-page-title">Assets</h1>
					<A href={`/spaces/${spaceId()}/entries`} class="text-sm">
						Back to Entries
					</A>
				</div>

				<AssetUploader assets={assets() || []} onUpload={handleUpload} onRemove={handleRemove} />

				<Show when={actionError()}>
					<p class="ui-alert ui-alert-error text-sm mt-4">{actionError()}</p>
				</Show>
				<Show when={assets.loading}>
					<p class="text-sm ui-muted mt-4">Loading assets...</p>
				</Show>
				<Show when={assets.error}>
					<p class="ui-alert ui-alert-error text-sm mt-4">Failed to load assets.</p>
				</Show>
			</div>
		</main>
	);
}
