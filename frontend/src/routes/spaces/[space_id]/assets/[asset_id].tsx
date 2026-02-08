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
		<main class="ui-shell ui-page">
			<div class="max-w-3xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="ui-page-title">Asset</h1>
					<A href={`/spaces/${spaceId()}/assets`} class="text-sm">
						Back to Assets
					</A>
				</div>

				<Show when={assets.loading}>
					<p class="text-sm ui-muted">Loading asset...</p>
				</Show>
				<Show when={assets.error}>
					<p class="ui-alert ui-alert-error text-sm">Failed to load asset.</p>
				</Show>
				<Show when={asset()}>
					{(item) => (
						<div class="ui-card">
							<p class="text-sm">Name: {item().name}</p>
							<p class="text-sm ui-muted">ID: {item().id}</p>
							<p class="text-sm ui-muted">Path: {item().path}</p>
							<button
								type="button"
								class="ui-button ui-button-danger mt-4"
								onClick={handleDelete}
								disabled={isDeleting()}
							>
								{isDeleting() ? "Deleting..." : "Delete Asset"}
							</button>
							<Show when={deleteError()}>
								<p class="ui-alert ui-alert-error text-sm mt-2">{deleteError()}</p>
							</Show>
						</div>
					)}
				</Show>
			</div>
		</main>
	);
}
