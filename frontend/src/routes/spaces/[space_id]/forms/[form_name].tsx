import { useParams, useNavigate } from "@solidjs/router";
import { Show, createEffect, createMemo, createResource, createSignal } from "solid-js";
import { FormTable } from "~/components/FormTable";
import { EditFormDialog } from "~/components/create-dialogs";
import { SpaceShell } from "~/components/SpaceShell";
import { useEntriesRouteContext } from "~/lib/entries-route-context";
import { formApi } from "~/lib/form-api";
import type { FormCreatePayload } from "~/lib/types";

export default function SpaceFormDetailRoute() {
	const params = useParams<{ form_name: string }>();
	const navigate = useNavigate();
	const ctx = useEntriesRouteContext();
	const [showEditDialog, setShowEditDialog] = createSignal(false);
	const [didRefetchForms, setDidRefetchForms] = createSignal(false);

	const decodedFormName = createMemo(() => {
		const raw = params.form_name;
		if (!raw) return "";
		try {
			return decodeURIComponent(raw);
		} catch {
			return raw;
		}
	});

	const formDef = createMemo(() => {
		const name = decodedFormName();
		return ctx.forms().find((s) => s.name === name);
	});

	const [fetchedForm] = createResource(
		() => {
			const name = decodedFormName();
			const spaceId = ctx.spaceId();
			if (!spaceId || !name) return null;
			if (formDef()) return null;
			return { spaceId, name };
		},
		async ({ spaceId, name }) => formApi.get(spaceId, name),
	);

	const resolvedForm = createMemo(() => formDef() ?? fetchedForm());
	const loadingForm = createMemo(() => ctx.loadingForms() || fetchedForm.loading);

	createEffect(() => {
		if (loadingForm()) return;
		if (resolvedForm()) return;
		if (didRefetchForms()) return;
		setDidRefetchForms(true);
		ctx.refetchForms();
	});

	const handleUpdateForm = async (payload: FormCreatePayload) => {
		try {
			await formApi.create(ctx.spaceId(), payload);
			setShowEditDialog(false);
			ctx.refetchForms();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to update form");
		}
	};

	return (
		<SpaceShell spaceId={ctx.spaceId()} showBottomTabs activeBottomTab="grid">
			<div class="mx-auto max-w-6xl">
				<Show
					when={resolvedForm()}
					keyed
					fallback={
						<div class="ui-card text-center ui-muted">
							{loadingForm() ? "Loading form..." : "Form not found"}
						</div>
					}
				>
					{(s) => (
						<div class="h-full flex flex-col">
							<div class="ui-card ui-card-header flex flex-wrap items-center justify-between gap-2 p-4">
								<h1 class="text-xl font-bold">{s.name}</h1>
								<button
									type="button"
									onClick={() => setShowEditDialog(true)}
									class="ui-button ui-button-secondary text-sm"
								>
									Edit Form
								</button>
							</div>
							<FormTable
								spaceId={ctx.spaceId()}
								entryForm={s}
								onEntryClick={(entryId) =>
									navigate(`/spaces/${ctx.spaceId()}/entries/${encodeURIComponent(entryId)}`)
								}
							/>
							<EditFormDialog
								open={showEditDialog()}
								entryForm={s}
								columnTypes={ctx.columnTypes()}
								formNames={ctx.forms().map((form) => form.name)}
								onClose={() => setShowEditDialog(false)}
								onSubmit={handleUpdateForm}
							/>
						</div>
					)}
				</Show>
			</div>
		</SpaceShell>
	);
}
