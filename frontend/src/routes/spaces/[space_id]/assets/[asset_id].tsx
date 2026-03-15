import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, Show } from "solid-js";
import { assetApi } from "~/lib/asset-api";
import { t } from "~/lib/i18n";

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
			setDeleteError(err instanceof Error ? err.message : t("assetDetail.failedDelete"));
		} finally {
			setIsDeleting(false);
		}
	};

	return (
		<main class="ui-shell ui-page">
			<div class="max-w-3xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="ui-page-title">{t("assetDetail.heading")}</h1>
					<A href={`/spaces/${spaceId()}/assets`} class="text-sm">
						{t("assetDetail.backToAssets")}
					</A>
				</div>

				<Show when={assets.loading}>
					<p class="text-sm ui-muted">{t("assetDetail.loading")}</p>
				</Show>
				<Show when={assets.error}>
					<p class="ui-alert ui-alert-error text-sm">{t("assetDetail.failedLoad")}</p>
				</Show>
				<Show when={asset()}>
					{(item) => (
						<div class="ui-card">
							<p class="text-sm">{t("assetDetail.name", { name: item().name })}</p>
							<p class="text-sm ui-muted">{t("assetDetail.id", { id: item().id })}</p>
							<p class="text-sm ui-muted">{t("assetDetail.path", { path: item().path })}</p>
							<button
								type="button"
								class="ui-button ui-button-danger mt-4"
								onClick={handleDelete}
								disabled={isDeleting()}
							>
								{isDeleting() ? t("assetDetail.deleting") : t("assetDetail.delete")}
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
