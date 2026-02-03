import { useParams, useNavigate } from "@solidjs/router";
import { Show, createEffect, createMemo, createResource, createSignal } from "solid-js";
import { ClassTable } from "~/components/ClassTable";
import { EditClassDialog } from "~/components/create-dialogs";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { useNotesRouteContext } from "~/lib/notes-route-context";
import { classApi } from "~/lib/class-api";
import type { ClassCreatePayload } from "~/lib/types";

export default function WorkspaceClassDetailRoute() {
	const params = useParams<{ class_name: string }>();
	const navigate = useNavigate();
	const ctx = useNotesRouteContext();
	const [showEditDialog, setShowEditDialog] = createSignal(false);
	const [didRefetchClasses, setDidRefetchClasses] = createSignal(false);

	const decodedClassName = createMemo(() => {
		const raw = params.class_name;
		if (!raw) return "";
		try {
			return decodeURIComponent(raw);
		} catch {
			return raw;
		}
	});

	const classDef = createMemo(() => {
		const name = decodedClassName();
		return ctx.classes().find((s) => s.name === name);
	});

	const [fetchedClass] = createResource(
		() => {
			const name = decodedClassName();
			const workspaceId = ctx.workspaceId();
			if (!workspaceId || !name) return null;
			if (classDef()) return null;
			return { workspaceId, name };
		},
		async ({ workspaceId, name }) => classApi.get(workspaceId, name),
	);

	const resolvedClass = createMemo(() => classDef() ?? fetchedClass());
	const loadingClass = createMemo(() => ctx.loadingClasses() || fetchedClass.loading);

	createEffect(() => {
		if (loadingClass()) return;
		if (resolvedClass()) return;
		if (didRefetchClasses()) return;
		setDidRefetchClasses(true);
		ctx.refetchClasses();
	});

	const handleUpdateClass = async (payload: ClassCreatePayload) => {
		try {
			await classApi.create(ctx.workspaceId(), payload);
			setShowEditDialog(false);
			ctx.refetchClasses();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to update class");
		}
	};

	return (
		<WorkspaceShell workspaceId={ctx.workspaceId()} showBottomTabs activeBottomTab="grid">
			<div class="mx-auto max-w-6xl">
				<Show
					when={resolvedClass()}
					keyed
					fallback={
						<div class="p-8 text-center text-gray-500">
							{loadingClass() ? "Loading class..." : "Class not found"}
						</div>
					}
				>
					{(s) => (
						<div class="h-full flex flex-col">
							<div class="flex flex-wrap items-center justify-between gap-2 p-4 border-b bg-white rounded-t-2xl">
								<h1 class="text-xl font-bold">{s.name}</h1>
								<button
									type="button"
									onClick={() => setShowEditDialog(true)}
									class="px-3 py-1 bg-white border border-gray-300 rounded text-sm hover:bg-gray-50 text-gray-700 font-medium"
								>
									Edit Class
								</button>
							</div>
							<ClassTable
								workspaceId={ctx.workspaceId()}
								noteClass={s}
								onNoteClick={(noteId) =>
									navigate(`/workspaces/${ctx.workspaceId()}/notes/${encodeURIComponent(noteId)}`)
								}
							/>
							<EditClassDialog
								open={showEditDialog()}
								noteClass={s}
								columnTypes={ctx.columnTypes()}
								onClose={() => setShowEditDialog(false)}
								onSubmit={handleUpdateClass}
							/>
						</div>
					)}
				</Show>
			</div>
		</WorkspaceShell>
	);
}
