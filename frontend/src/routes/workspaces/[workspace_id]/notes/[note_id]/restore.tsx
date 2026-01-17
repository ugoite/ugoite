import { A, useNavigate, useParams } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { noteApi } from "~/lib/note-api";

export default function WorkspaceNoteRestoreRoute() {
	const navigate = useNavigate();
	const params = useParams<{ workspace_id: string; note_id: string }>();
	const workspaceId = () => params.workspace_id;
	const noteId = () => params.note_id;
	const [selectedRevision, setSelectedRevision] = createSignal<string | null>(null);
	const [restoreError, setRestoreError] = createSignal<string | null>(null);
	const [isRestoring, setIsRestoring] = createSignal(false);

	const [history] = createResource(async () => {
		return await noteApi.history(workspaceId(), noteId());
	});

	const handleRestore = async () => {
		const revisionId = selectedRevision();
		if (!revisionId) return;
		setIsRestoring(true);
		setRestoreError(null);
		try {
			await noteApi.restore(workspaceId(), noteId(), revisionId);
			navigate(`/workspaces/${workspaceId()}/notes/${noteId()}`);
		} catch (err) {
			setRestoreError(err instanceof Error ? err.message : "Failed to restore note");
		} finally {
			setIsRestoring(false);
		}
	};

	return (
		<main class="flex-1 overflow-auto p-6">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="text-xl font-semibold text-gray-900">Restore Note</h1>
					<p class="text-sm text-gray-500">Select a revision to restore.</p>
				</div>
				<A
					href={`/workspaces/${workspaceId()}/notes/${noteId()}`}
					class="text-sm text-sky-700 hover:underline"
				>
					Back to Note
				</A>
			</div>

			<Show when={history.loading}>
				<p class="text-sm text-gray-500">Loading revisions...</p>
			</Show>
			<Show when={history.error}>
				<p class="text-sm text-red-600">Failed to load history.</p>
			</Show>
			<Show when={history()}>
				{(data) => (
					<div class="space-y-4">
						<ul class="space-y-2">
							<For each={data().revisions}>
								{(revision) => (
									<li class="flex items-center gap-2">
										<input
											type="radio"
											name="revision"
											value={revision.revision_id}
											checked={selectedRevision() === revision.revision_id}
											onChange={() => setSelectedRevision(revision.revision_id)}
										/>
										<span class="text-sm text-gray-800">{revision.revision_id}</span>
										<span class="text-xs text-gray-500">{revision.created_at}</span>
									</li>
								)}
							</For>
						</ul>
						<button
							type="button"
							class="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50"
							onClick={handleRestore}
							disabled={!selectedRevision() || isRestoring()}
						>
							{isRestoring() ? "Restoring..." : "Restore Selected Revision"}
						</button>
						<Show when={restoreError()}>
							<p class="text-sm text-red-600">{restoreError()}</p>
						</Show>
					</div>
				)}
			</Show>
		</main>
	);
}
