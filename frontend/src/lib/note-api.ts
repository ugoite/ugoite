import type { Note, NoteCreatePayload, NoteRecord, NoteRevision, NoteUpdatePayload } from "./types";
import { apiFetch } from "./api";

/**
 * Note API client
 */
export const noteApi = {
	/** List all notes in a workspace (uses index) */
	async list(workspaceId: string): Promise<NoteRecord[]> {
		const res = await apiFetch(`/workspaces/${encodeURIComponent(workspaceId)}/notes`);
		if (!res.ok) {
			throw new Error(`Failed to list notes: ${res.statusText}`);
		}
		return (await res.json()) as NoteRecord[];
	},

	/** Get a single note */
	async get(workspaceId: string, noteId: string): Promise<Note> {
		const res = await apiFetch(
			`/workspaces/${encodeURIComponent(workspaceId)}/notes/${encodeURIComponent(noteId)}`,
		);
		if (!res.ok) {
			let detail = res.statusText;
			try {
				const data = (await res.json()) as { detail?: string };
				if (data?.detail) {
					detail = data.detail;
				}
			} catch {
				// ignore parse errors
			}
			throw new Error(`Failed to get note: ${detail}`);
		}
		return (await res.json()) as Note;
	},

	/** Create a new note */
	async create(
		workspaceId: string,
		payload: NoteCreatePayload,
	): Promise<{ id: string; revision_id: string }> {
		const res = await apiFetch(`/workspaces/${encodeURIComponent(workspaceId)}/notes`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create note: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; revision_id: string };
	},

	/** Update a note (requires parent_revision_id for optimistic locking) */
	async update(
		workspaceId: string,
		noteId: string,
		payload: NoteUpdatePayload,
	): Promise<{ id: string; revision_id: string }> {
		const res = await apiFetch(
			`/workspaces/${encodeURIComponent(workspaceId)}/notes/${encodeURIComponent(noteId)}`,
			{
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify(payload),
			},
		);
		if (!res.ok) {
			const error = (await res.json()) as {
				detail?: string;
				current_revision_id?: string;
			};
			if (res.status === 409) {
				throw new RevisionConflictError(
					error.detail || "Revision mismatch",
					error.current_revision_id,
				);
			}
			throw new Error(error.detail || `Failed to update note: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; revision_id: string };
	},

	/** Delete a note */
	async delete(workspaceId: string, noteId: string): Promise<void> {
		const res = await apiFetch(
			`/workspaces/${encodeURIComponent(workspaceId)}/notes/${encodeURIComponent(noteId)}`,
			{
				method: "DELETE",
			},
		);
		if (!res.ok) {
			throw new Error(`Failed to delete note: ${res.statusText}`);
		}
	},

	/** Get note revision history */
	async history(workspaceId: string, noteId: string): Promise<{ revisions: NoteRevision[] }> {
		const res = await apiFetch(
			`/workspaces/${encodeURIComponent(workspaceId)}/notes/${encodeURIComponent(noteId)}/history`,
		);
		if (!res.ok) {
			throw new Error(`Failed to get note history: ${res.statusText}`);
		}
		return (await res.json()) as { revisions: NoteRevision[] };
	},

	/** Get a specific note revision */
	async getRevision(workspaceId: string, noteId: string, revisionId: string): Promise<Note> {
		const res = await apiFetch(
			`/workspaces/${encodeURIComponent(workspaceId)}/notes/${encodeURIComponent(noteId)}/history/${encodeURIComponent(revisionId)}`,
		);
		if (!res.ok) {
			throw new Error(`Failed to get note revision: ${res.statusText}`);
		}
		return (await res.json()) as Note;
	},

	/** Restore note to a previous revision */
	async restore(workspaceId: string, noteId: string, revisionId: string): Promise<Note> {
		const res = await apiFetch(
			`/workspaces/${encodeURIComponent(workspaceId)}/notes/${encodeURIComponent(noteId)}/restore`,
			{
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ revision_id: revisionId }),
			},
		);
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to restore note: ${res.statusText}`);
		}
		return (await res.json()) as Note;
	},
};

/** Custom error for revision conflicts (409) */
export class RevisionConflictError extends Error {
	constructor(
		message: string,
		public currentRevisionId?: string,
	) {
		super(message);
		this.name = "RevisionConflictError";
	}
}
