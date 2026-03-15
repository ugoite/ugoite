import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, Show } from "solid-js";
import { AssetUploader } from "~/components/AssetUploader";
import { CreateEntryDialog, CreateFormDialog } from "~/components/create-dialogs";
import { assetApi } from "~/lib/asset-api";
import { SpaceShell } from "~/components/SpaceShell";
import { createEntryStore } from "~/lib/entry-store";
import { buildEntryMarkdownByMode, type EntryInputMode } from "~/lib/entry-input";
import { t } from "~/lib/i18n";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import { formApi } from "~/lib/form-api";
import { spaceApi } from "~/lib/space-api";
import type { FormCreatePayload } from "~/lib/types";
import type { Asset } from "~/lib/types";

export default function SpaceDashboardRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;
	const navigate = useNavigate();
	const entryStore = createEntryStore(spaceId);
	const [showCreateEntryDialog, setShowCreateEntryDialog] = createSignal(false);
	const [showCreateFormDialog, setShowCreateFormDialog] = createSignal(false);
	const [assetActionError, setAssetActionError] = createSignal<string | null>(null);

	const [space] = createResource(async () => {
		return await spaceApi.get(spaceId());
	});

	const [forms, { refetch: refetchForms }] = createResource(
		() => spaceId(),
		async (wsId) => {
			if (!wsId) return [];
			return await formApi.list(wsId);
		},
	);

	const [columnTypes] = createResource(
		() => spaceId(),
		async (wsId) => {
			if (!wsId) return [];
			return await formApi.listTypes(wsId);
		},
	);

	const [assets, { refetch: refetchAssets }] = createResource(
		() => spaceId(),
		async (wsId) => {
			if (!wsId) return [] as Asset[];
			return await assetApi.list(wsId);
		},
	);

	const safeForms = createMemo(() => forms() || []);
	const entryForms = createMemo(() => filterCreatableEntryForms(safeForms()));
	const displaySpaceName = createMemo(() => space()?.name || spaceId());

	const handleCreateForm = async (payload: FormCreatePayload) => {
		try {
			await formApi.create(spaceId(), payload);
			setShowCreateFormDialog(false);
			await refetchForms();
		} catch (e) {
			alert(e instanceof Error ? e.message : t("dashboard.error.failedCreateForm"));
		}
	};

	const handleCreateEntry = async (
		title: string,
		formName: string,
		requiredValues: Record<string, string>,
		inputMode: EntryInputMode = "webform",
	) => {
		if (!formName) {
			alert(t("dashboard.error.selectFormBeforeCreate"));
			return;
		}
		const formDef = entryForms().find((entryForm) => entryForm.name === formName);
		if (!formDef) {
			alert(t("dashboard.error.selectedFormNotFound"));
			return;
		}
		const initialContent = buildEntryMarkdownByMode(formDef, title, requiredValues, inputMode);

		try {
			const result = await entryStore.createEntry(initialContent);
			setShowCreateEntryDialog(false);
			navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(result.id)}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : t("dashboard.error.failedCreateEntry"));
		}
	};

	const handleAssetUpload = async (file: File): Promise<Asset> => {
		setAssetActionError(null);
		const created = await assetApi.upload(spaceId(), file, file.name);
		await refetchAssets();
		return created;
	};

	const handleAssetRemove = async (assetId: string) => {
		setAssetActionError(null);
		try {
			await assetApi.delete(spaceId(), assetId);
			await refetchAssets();
		} catch (error) {
			setAssetActionError(
				error instanceof Error ? error.message : t("dashboard.error.failedDeleteAsset"),
			);
		}
	};

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="dashboard">
			<div class="mx-auto max-w-5xl ui-stack">
				<div>
					<h1 class="ui-page-title text-3xl sm:text-4xl">{displaySpaceName()}</h1>
					<Show when={space.error}>
						<p class="text-sm ui-text-danger">{t("dashboard.error.failedLoadSpace")}</p>
					</Show>
				</div>

				<div class="grid gap-4 sm:grid-cols-2">
					<section class="ui-card ui-stack-sm">
						<div>
							<h2 class="text-lg font-semibold">{t("dashboard.section.createEntry.heading")}</h2>
							<Show
								when={!forms.loading}
								fallback={
									<p class="text-sm ui-muted">{t("dashboard.section.createEntry.loading")}</p>
								}
							>
								<p class="text-sm ui-muted">
									{entryForms().length > 0
										? t("dashboard.section.createEntry.formsAvailable", {
												count: entryForms().length,
											})
										: t("dashboard.section.createEntry.empty")}
								</p>
							</Show>
						</div>
						<div class="flex flex-wrap gap-2">
							<button
								type="button"
								class="ui-button ui-button-primary text-sm"
								disabled={entryForms().length === 0}
								onClick={() => setShowCreateEntryDialog(true)}
							>
								{t("dashboard.section.createEntry.new")}
							</button>
							<A
								href={`/spaces/${spaceId()}/entries`}
								class="ui-button ui-button-secondary text-sm"
							>
								{t("dashboard.section.createEntry.browse")}
							</A>
						</div>
					</section>
					<section class="ui-card ui-stack-sm">
						<div>
							<h2 class="text-lg font-semibold">{t("dashboard.section.createForm.heading")}</h2>
							<p class="text-sm ui-muted">{t("dashboard.section.createForm.description")}</p>
						</div>
						<div class="flex flex-wrap gap-2">
							<button
								type="button"
								class="ui-button ui-button-primary text-sm"
								onClick={() => setShowCreateFormDialog(true)}
							>
								{t("dashboard.section.createForm.new")}
							</button>
							<A href={`/spaces/${spaceId()}/forms`} class="ui-button ui-button-secondary text-sm">
								{t("dashboard.section.createForm.browse")}
							</A>
						</div>
					</section>
					<section class="ui-card ui-stack-sm">
						<div>
							<h2 class="text-lg font-semibold">{t("dashboard.section.assets.heading")}</h2>
							<p class="text-sm ui-muted">{t("dashboard.section.assets.description")}</p>
						</div>
						<AssetUploader
							assets={assets() || []}
							onUpload={handleAssetUpload}
							onRemove={handleAssetRemove}
						/>
						<Show when={assetActionError()}>
							<p class="ui-alert ui-alert-error text-sm">{assetActionError()}</p>
						</Show>
						<Show when={assets.loading}>
							<p class="text-sm ui-muted">{t("dashboard.section.assets.loading")}</p>
						</Show>
					</section>
				</div>
			</div>

			<CreateEntryDialog
				open={showCreateEntryDialog()}
				forms={entryForms()}
				onClose={() => setShowCreateEntryDialog(false)}
				onSubmit={handleCreateEntry}
			/>
			<CreateFormDialog
				open={showCreateFormDialog()}
				columnTypes={columnTypes() || []}
				formNames={safeForms().map((form) => form.name)}
				onClose={() => setShowCreateFormDialog(false)}
				onSubmit={handleCreateForm}
			/>
		</SpaceShell>
	);
}
