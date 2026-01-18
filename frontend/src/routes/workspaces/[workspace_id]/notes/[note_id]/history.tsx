import { A, useParams } from "@solidjs/router";
import { createResource, For, Show } from "solid-js";
import { noteApi } from "~/lib/note-api";

export default function WorkspaceNoteHistoryRoute() {
	const params = useParams<{ workspace_id: string; note_id: string }>();
	const workspaceId = () => params.workspace_id;
	const noteId = () => params.note_id;
	const encodedNoteId = () => encodeURIComponent(noteId());

	const [history] = createResource(async () => {
		return await noteApi.history(workspaceId(), noteId());
	});

	return (
		<main class="flex-1 overflow-auto p-6">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="text-xl font-semibold text-gray-900">Note History</h1>
					<p class="text-sm text-gray-500">Note ID: {noteId()}</p>
				</div>
				<A
					href={`/workspaces/${workspaceId()}/notes/${encodedNoteId()}`}
					class="text-sm text-sky-700 hover:underline"
				>
					Back to Note
				</A>
			</div>

			<Show when={history.loading}>
				<p class="text-sm text-gray-500">Loading history...</p>
			</Show>
			<Show when={history.error}>
				<p class="text-sm text-red-600">Failed to load history.</p>
			</Show>
			<Show when={history()}>
				{(data) => (
					<ul class="space-y-3">
						<For each={data().revisions}>
							{(revision) => (
								<li class="border rounded-lg p-4 flex items-center justify-between">
									<div>
										<p class="text-sm font-medium text-gray-800">
											Revision: {revision.revision_id}
										</p>
										<p class="text-xs text-gray-500">{revision.created_at}</p>
									</div>
									<A
										href={`/workspaces/${workspaceId()}/notes/${encodedNoteId()}/history/${encodeURIComponent(revision.revision_id)}`}
										class="text-sm text-blue-600 hover:underline"
									>
										View Revision
									</A>
								</li>
							)}
						</For>
					</ul>
				)}
			</Show>
		</main>
	);
}
