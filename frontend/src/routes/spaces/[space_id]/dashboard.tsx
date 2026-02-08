import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, Show } from "solid-js";
import { CreateEntryDialog, CreateFormDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { createEntryStore } from "~/lib/entry-store";
import { formApi } from "~/lib/form-api";
import { ensureFormFrontmatter, replaceFirstH1, updateH2Section } from "~/lib/markdown";
import { spaceApi } from "~/lib/space-api";
import type { FormCreatePayload } from "~/lib/types";

export default function SpaceDashboardRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;
	const navigate = useNavigate();
	const entryStore = createEntryStore(spaceId);
	const [showCreateEntryDialog, setShowCreateEntryDialog] = createSignal(false);
	const [showCreateFormDialog, setShowCreateFormDialog] = createSignal(false);

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

	const safeForms = createMemo(() => forms() || []);

	const handleCreateForm = async (payload: FormCreatePayload) => {
		try {
			await formApi.create(spaceId(), payload);
			setShowCreateFormDialog(false);
			await refetchForms();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create form");
		}
	};

	const handleCreateEntry = async (
		title: string,
		formName: string,
		requiredValues: Record<string, string>,
	) => {
		if (!formName) {
			alert("Please select a form to create a entry.");
			return;
		}
		const formDef = safeForms().find((entryForm) => entryForm.name === formName);
		if (!formDef) {
			alert("Selected form was not found. Please refresh and try again.");
			return;
		}
		let initialContent = ensureFormFrontmatter(replaceFirstH1(formDef.template, title), formName);
		for (const [name, value] of Object.entries(requiredValues)) {
			if (!value.trim()) continue;
			initialContent = updateH2Section(initialContent, name, value.trim());
		}

		try {
			const result = await entryStore.createEntry(initialContent);
			setShowCreateEntryDialog(false);
			navigate(`/spaces/${spaceId()}/entries/${encodeURIComponent(result.id)}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create entry");
		}
	};

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="dashboard">
			<div class="mx-auto max-w-5xl ui-stack">
				<div>
					<Show when={space.loading}>
						<p class="text-sm ui-muted">Loading space...</p>
					</Show>
					<Show when={space.error}>
						<p class="text-sm ui-text-danger">Failed to load space.</p>
					</Show>
					<Show when={space()}>
						{(ws) => <h1 class="ui-page-title text-3xl sm:text-4xl">{ws().name}</h1>}
					</Show>
				</div>

				<div class="grid gap-4 sm:grid-cols-2">
					<section class="ui-card ui-stack-sm">
						<div>
							<h2 class="text-lg font-semibold">Create entry</h2>
							<Show
								when={!forms.loading}
								fallback={<p class="text-sm ui-muted">Loading forms...</p>}
							>
								<p class="text-sm ui-muted">
									{safeForms().length > 0
										? `${safeForms().length} forms available`
										: "Create a form first to start writing entries."}
								</p>
							</Show>
						</div>
						<div class="flex flex-wrap gap-2">
							<button
								type="button"
								class="ui-button ui-button-primary text-sm"
								onClick={() => setShowCreateEntryDialog(true)}
							>
								New entry
							</button>
							<A
								href={`/spaces/${spaceId()}/entries`}
								class="ui-button ui-button-secondary text-sm"
							>
								Browse entries
							</A>
						</div>
					</section>
					<section class="ui-card ui-stack-sm">
						<div>
							<h2 class="text-lg font-semibold">Create form</h2>
							<p class="text-sm ui-muted">Define fields once and reuse them in entries.</p>
						</div>
						<div class="flex flex-wrap gap-2">
							<button
								type="button"
								class="ui-button ui-button-primary text-sm"
								onClick={() => setShowCreateFormDialog(true)}
							>
								New form
							</button>
							<A href={`/spaces/${spaceId()}/forms`} class="ui-button ui-button-secondary text-sm">
								Browse forms
							</A>
						</div>
					</section>
				</div>
			</div>

			<CreateEntryDialog
				open={showCreateEntryDialog()}
				forms={safeForms()}
				onClose={() => setShowCreateEntryDialog(false)}
				onSubmit={handleCreateEntry}
			/>
			<CreateFormDialog
				open={showCreateFormDialog()}
				columnTypes={columnTypes() || []}
				onClose={() => setShowCreateFormDialog(false)}
				onSubmit={handleCreateForm}
			/>
		</SpaceShell>
	);
}
