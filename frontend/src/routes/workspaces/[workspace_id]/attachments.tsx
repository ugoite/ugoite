import { A, useParams } from "@solidjs/router";
import { createResource, createSignal, Show } from "solid-js";
import { AttachmentUploader } from "~/components/AttachmentUploader";
import { attachmentApi } from "~/lib/attachment-api";
import type { Attachment } from "~/lib/types";

export default function WorkspaceAttachmentsRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;
	const [actionError, setActionError] = createSignal<string | null>(null);

	const [attachments, { refetch }] = createResource(async () => {
		return await attachmentApi.list(workspaceId());
	});

	const handleUpload = async (file: File): Promise<Attachment> => {
		setActionError(null);
		const created = await attachmentApi.upload(workspaceId(), file, file.name);
		await refetch();
		return created;
	};

	const handleRemove = async (attachmentId: string) => {
		setActionError(null);
		try {
			await attachmentApi.delete(workspaceId(), attachmentId);
			await refetch();
		} catch (err) {
			setActionError(err instanceof Error ? err.message : "Failed to delete attachment");
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-4xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="text-2xl font-bold text-gray-900">Attachments</h1>
					<A
						href={`/workspaces/${workspaceId()}/notes`}
						class="text-sm text-sky-700 hover:underline"
					>
						Back to Notes
					</A>
				</div>

				<AttachmentUploader
					attachments={attachments() || []}
					onUpload={handleUpload}
					onRemove={handleRemove}
				/>

				<Show when={actionError()}>
					<p class="text-sm text-red-600 mt-4">{actionError()}</p>
				</Show>
				<Show when={attachments.loading}>
					<p class="text-sm text-gray-500 mt-4">Loading attachments...</p>
				</Show>
				<Show when={attachments.error}>
					<p class="text-sm text-red-600 mt-4">Failed to load attachments.</p>
				</Show>
			</div>
		</main>
	);
}
