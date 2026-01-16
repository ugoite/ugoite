import { createSignal } from "solid-js";
import type { Attachment } from "./types";
import { attachmentApi } from "./attachment-api";

export function createAttachmentStore(workspaceId: () => string) {
	const [attachments, setAttachments] = createSignal<Attachment[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function loadAttachments(): Promise<void> {
		setLoading(true);
		setError(null);
		try {
			const data = await attachmentApi.list(workspaceId());
			setAttachments(data);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load attachments");
		} finally {
			setLoading(false);
		}
	}

	async function uploadAttachment(file: File | Blob, filename?: string): Promise<Attachment> {
		setError(null);
		const uploaded = await attachmentApi.upload(workspaceId(), file, filename);
		await loadAttachments();
		return uploaded;
	}

	async function deleteAttachment(attachmentId: string): Promise<void> {
		setError(null);
		await attachmentApi.delete(workspaceId(), attachmentId);
		await loadAttachments();
	}

	return {
		attachments,
		loading,
		error,
		loadAttachments,
		uploadAttachment,
		deleteAttachment,
	};
}

export type AttachmentStore = ReturnType<typeof createAttachmentStore>;
