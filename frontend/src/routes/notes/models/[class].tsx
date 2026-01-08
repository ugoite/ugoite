import { useParams, useNavigate } from "@solidjs/router";
import { Show, createMemo, createSignal } from "solid-js";
import { SchemaTable } from "~/components/SchemaTable";
import { EditSchemaDialog } from "~/components/create-dialogs";
import { useNotesRouteContext } from "~/lib/notes-route-context";
import { schemaApi } from "~/lib/client";
import type { SchemaCreatePayload } from "~/lib/types";

export default function SchemaRoute() {
	const params = useParams();
	const navigate = useNavigate();
	const ctx = useNotesRouteContext();
	const [showEditDialog, setShowEditDialog] = createSignal(false);

	const schema = createMemo(() => {
		const name = params.class;
		return ctx.schemas().find((s) => s.name === name);
	});

	const handleUpdateSchema = async (payload: SchemaCreatePayload) => {
		try {
			await schemaApi.create(ctx.workspaceId(), payload);
			setShowEditDialog(false);
			ctx.refetchSchemas();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to update class");
		}
	};

	return (
		<Show
			when={schema()}
			keyed
			fallback={
				<div class="p-8 text-center text-gray-500">
					{ctx.loadingSchemas() ? "Loading data models..." : "Data model not found"}
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
					<SchemaTable
						workspaceId={ctx.workspaceId()}
						schema={s}
						onNoteClick={(noteId) => navigate(`/notes/${noteId}`)}
					/>
					<EditSchemaDialog
						open={showEditDialog()}
						schema={s}
						columnTypes={ctx.columnTypes()}
						onClose={() => setShowEditDialog(false)}
						onSubmit={handleUpdateSchema}
					/>
				</div>
			)}
		</Show>
	);
}
