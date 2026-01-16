import type { Attachment } from "./types";
import { apiFetch } from "./api";

/** Attachment API client */
export const attachmentApi = {
	/** Upload an attachment */
	async upload(workspaceId: string, file: File | Blob, filename?: string): Promise<Attachment> {
		const formData = new FormData();
		formData.append("file", file, filename);
		const res = await apiFetch(`/workspaces/${workspaceId}/attachments`, {
			method: "POST",
			body: formData,
		});
		if (!res.ok) {
			throw new Error(`Failed to upload attachment: ${res.statusText}`);
		}
		return (await res.json()) as Attachment;
	},

	/** List all attachments in workspace */
	async list(workspaceId: string): Promise<Attachment[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/attachments`);
		if (!res.ok) {
			throw new Error(`Failed to list attachments: ${res.statusText}`);
		}
		return (await res.json()) as Attachment[];
	},

	/** Delete an attachment (fails if referenced) */
	async delete(workspaceId: string, attachmentId: string): Promise<{ status: string; id: string }> {
		const res = await apiFetch(`/workspaces/${workspaceId}/attachments/${attachmentId}`, {
			method: "DELETE",
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to delete attachment: ${res.statusText}`);
		}
		return (await res.json()) as { status: string; id: string };
	},
};
