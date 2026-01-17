import { A, useParams } from "@solidjs/router";
import { createResource, Show } from "solid-js";
import { noteApi } from "~/lib/note-api";

export default function WorkspaceNoteRevisionRoute() {
	const params = useParams<{ workspace_id: string; note_id: string; revision_id: string }>();
	const workspaceId = () => params.workspace_id;
	const noteId = () => params.note_id;
	const revisionId = () => params.revision_id;

	const [revision] = createResource(async () => {
		return await noteApi.getRevision(workspaceId(), noteId(), revisionId());
	});

	return (
		<main class="flex-1 overflow-auto p-6">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="text-xl font-semibold text-gray-900">Note Revision</h1>
					<p class="text-sm text-gray-500">Revision ID: {revisionId()}</p>
				</div>
				<A
					href={`/workspaces/${workspaceId()}/notes/${noteId()}/history`}
					class="text-sm text-sky-700 hover:underline"
				>
					Back to History
				</A>
			</div>

			<Show when={revision.loading}>
				<p class="text-sm text-gray-500">Loading revision...</p>
			</Show>
			<Show when={revision.error}>
				<p class="text-sm text-red-600">Failed to load revision.</p>
			</Show>
			<Show when={revision()}>
				{(note) => (
					<div class="bg-white border rounded-lg p-4">
						<h2 class="text-lg font-semibold mb-2">{note().title}</h2>
						<pre class="text-sm whitespace-pre-wrap text-gray-700">{note().content}</pre>
					</div>
				)}
			</Show>
		</main>
	);
}
