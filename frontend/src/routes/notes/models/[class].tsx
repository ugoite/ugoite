import { useParams, useNavigate } from "@solidjs/router";
import { Show, createMemo } from "solid-js";
import { SchemaTable } from "~/components/SchemaTable";
import { useNotesRouteContext } from "~/lib/notes-route-context";

export default function SchemaRoute() {
	const params = useParams();
	const navigate = useNavigate();
	const ctx = useNotesRouteContext();

	const schema = createMemo(() => {
		const name = params.class;
		return ctx.schemas().find((s) => s.name === name);
	});

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
				<SchemaTable
					workspaceId={ctx.workspaceId()}
					schema={s}
					onNoteClick={(noteId) => navigate(`/notes/${noteId}`)}
				/>
			)}
		</Show>
	);
}
