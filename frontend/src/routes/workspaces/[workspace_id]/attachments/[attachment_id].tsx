import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createResource, createSignal, Show } from "solid-js";
import { attachmentApi } from "~/lib/attachment-api";

export default function WorkspaceAttachmentDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ workspace_id: string; attachment_id: string }>();
	const workspaceId = () => params.workspace_id;
	const attachmentId = () => params.attachment_id;
	const [deleteError, setDeleteError] = createSignal<string | null>(null);
	const [isDeleting, setIsDeleting] = createSignal(false);

	const [attachments] = createResource(async () => {
		return await attachmentApi.list(workspaceId());
	});

	const attachment = createMemo(() => {
		return attachments()?.find((item) => item.id === attachmentId()) || null;
	});

	const handleDelete = async () => {
		setDeleteError(null);
		setIsDeleting(true);
		try {
			await attachmentApi.delete(workspaceId(), attachmentId());
			navigate(`/workspaces/${workspaceId()}/attachments`);
		} catch (err) {
			setDeleteError(err instanceof Error ? err.message : "Failed to delete attachment");
		} finally {
			setIsDeleting(false);
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-3xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="text-2xl font-bold text-gray-900">Attachment</h1>
					<A
						href={`/workspaces/${workspaceId()}/attachments`}
						class="text-sm text-sky-700 hover:underline"
					>
						Back to Attachments
					</A>
				</div>

				<Show when={attachments.loading}>
					<p class="text-sm text-gray-500">Loading attachment...</p>
				</Show>
				<Show when={attachments.error}>
					<p class="text-sm text-red-600">Failed to load attachment.</p>
				</Show>
				<Show when={attachment()}>
					{(item) => (
						<div class="bg-white border rounded-lg p-4">
							<p class="text-sm text-gray-700">Name: {item().name}</p>
							<p class="text-sm text-gray-500">ID: {item().id}</p>
							<p class="text-sm text-gray-500">Path: {item().path}</p>
							<button
								type="button"
								class="mt-4 px-4 py-2 bg-red-600 text-white rounded hover:bg-red-700 disabled:opacity-50"
								onClick={handleDelete}
								disabled={isDeleting()}
							>
								{isDeleting() ? "Deleting..." : "Delete Attachment"}
							</button>
							<Show when={deleteError()}>
								<p class="text-sm text-red-600 mt-2">{deleteError()}</p>
							</Show>
						</div>
					)}
				</Show>
			</div>
		</main>
	);
}
