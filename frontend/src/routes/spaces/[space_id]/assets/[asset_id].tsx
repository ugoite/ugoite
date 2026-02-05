import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, Show } from "solid-js";
import { assetApi } from "~/lib/asset-api";

export default function SpaceAssetDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ space_id: string; asset_id: string }>();
	const spaceId = () => params.space_id;
	const assetId = () => params.asset_id;
	const [deleteError, setDeleteError] = createSignal<string | null>(null);
	const [isDeleting, setIsDeleting] = createSignal(false);

	const [assets] = createResource(async () => {
		return await assetApi.list(spaceId());
	});

	const asset = createMemo(() => {
		return assets()?.find((item) => item.id === assetId()) || null;
	});

	const handleDelete = async () => {
		setDeleteError(null);
		setIsDeleting(true);
		try {
			await assetApi.delete(spaceId(), assetId());
			navigate(`/spaces/${spaceId()}/assets`);
		} catch (err) {
			setDeleteError(err instanceof Error ? err.message : "Failed to delete asset");
		} finally {
			setIsDeleting(false);
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-3xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="text-2xl font-bold text-gray-900">Asset</h1>
					<A href={`/spaces/${spaceId()}/assets`} class="text-sm text-sky-700 hover:underline">
						Back to Assets
					</A>
				</div>

				<Show when={assets.loading}>
					<p class="text-sm text-gray-500">Loading asset...</p>
				</Show>
				<Show when={assets.error}>
					<p class="text-sm text-red-600">Failed to load asset.</p>
				</Show>
				<Show when={asset()}>
					{(item) => (
						<div class="bg-white border rounded-lg p-4">
							<p class="text-sm text-gray-700">Name: {item().name}</p>
							<p class="text-sm text-gray-500">ID: {item().id}</p>
							<p class="text-sm text-gray-500">Path: {item().path}</p>
							<button
								type="button"
								class="mt-4 px-4 py-2 bg-red-600 text-white rounded hover:bg-red-700 disabled:opacity-50"
								onClick={handleDelete}
								disabled={isDeleting()}
							>
								{isDeleting() ? "Deleting..." : "Delete Asset"}
							</button>
							<Show when={deleteError()}>
								<p class="text-sm text-red-600 mt-2">{deleteError()}</p>
							</Show>
						</div>
					)}
				</Show>
			</div>
		</main>
	);
}
