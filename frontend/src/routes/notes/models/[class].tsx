import { useParams, useNavigate } from "@solidjs/router";
import { Show, createResource } from "solid-js";
import { SchemaTable } from "~/components/SchemaTable";
import { schemaApi } from "~/lib/client";
import { createWorkspaceStore } from "~/lib/workspace-store";

export default function SchemaRoute() {
	const params = useParams();
	const navigate = useNavigate();
	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const [schema] = createResource(
		() => {
			const wsId = workspaceId();
			const name = params.class;
			return wsId && name ? { wsId, name } : null;
		},
		async ({ wsId, name }) => {
			const schemas = await schemaApi.list(wsId);
			return schemas.find((s) => s.name === name);
		},
	);

	return (
		<Show
			when={schema()}
			fallback={<div class="p-8 text-center text-gray-500">Loading data model...</div>}
		>
			{(s) => (
				<SchemaTable
					workspaceId={workspaceId()}
					schema={s()}
					onNoteClick={(noteId) => navigate(`/notes/${noteId}`)}
				/>
			)}
		</Show>
	);
}
