import { useParams, useNavigate } from "@solidjs/router";
import { Show, createMemo, createSignal } from "solid-js";
import { ClassTable } from "~/components/ClassTable";
import { EditClassDialog } from "~/components/create-dialogs";
import { useNotesRouteContext } from "~/lib/notes-route-context";
import { classApi } from "~/lib/client";
import type { ClassCreatePayload } from "~/lib/types";

export default function ClassRoute() {
	const params = useParams();
	const navigate = useNavigate();
	const ctx = useNotesRouteContext();
	const [showEditDialog, setShowEditDialog] = createSignal(false);

	const class_def = createMemo(() => {
		const name = params.class;
		return ctx.classes().find((s) => s.name === name);
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
		<Show
			when={class_def()}
			keyed
			fallback={
				<div class="p-8 text-center text-gray-500">
					{ctx.loadingClasses() ? "Loading data models..." : "Note class not found"}
				</div>
			}
		>
			{(s) => (
				<div class="h-full flex flex-col">
					<div class="flex items-center justify-between p-4 border-b bg-white">
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
						onNoteClick={(noteId) => navigate(`/notes/${noteId}`)}
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
	);
}
