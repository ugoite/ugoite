import type {
	Note,
	NoteCreatePayload,
	NoteRecord,
	NoteUpdatePayload,
	Workspace,
} from "./types";
import { apiFetch } from "./api";

/**
 * Workspace API client
 */
export const workspaceApi = {
	/** List all workspaces */
	async list(): Promise<Workspace[]> {
		const res = await apiFetch("/workspaces");
		if (!res.ok) {
			throw new Error(`Failed to list workspaces: ${res.statusText}`);
		}
		return (await res.json()) as Workspace[];
	},

	/** Create a new workspace */
	async create(name: string): Promise<{ id: string; name: string }> {
		const res = await apiFetch("/workspaces", {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ name }),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create workspace: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; name: string };
	},

	/** Get workspace by ID */
	async get(id: string): Promise<Workspace> {
		const res = await apiFetch(`/workspaces/${id}`);
		if (!res.ok) {
			throw new Error(`Failed to get workspace: ${res.statusText}`);
		}
		return (await res.json()) as Workspace;
	},
};

/**
 * Note API client
 */
export const noteApi = {
	/** List all notes in a workspace (uses index) */
	async list(workspaceId: string): Promise<NoteRecord[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/notes`);
		if (!res.ok) {
			throw new Error(`Failed to list notes: ${res.statusText}`);
		}
		return (await res.json()) as NoteRecord[];
	},

	/** Get a single note */
	async get(workspaceId: string, noteId: string): Promise<Note> {
		const res = await apiFetch(`/workspaces/${workspaceId}/notes/${noteId}`);
		if (!res.ok) {
			throw new Error(`Failed to get note: ${res.statusText}`);
		}
		return (await res.json()) as Note;
	},

	/** Create a new note */
	async create(
		workspaceId: string,
		payload: NoteCreatePayload,
	): Promise<{ id: string; revision_id: string }> {
		const res = await apiFetch(`/workspaces/${workspaceId}/notes`, {
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
		const res = await apiFetch(`/workspaces/${workspaceId}/notes/${noteId}`, {
			method: "PUT",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as {
				detail?: string;
				current_revision_id?: string;
			};
			if (res.status === 409) {
				throw new RevisionConflictError(error.detail || "Revision mismatch", error.current_revision_id);
			}
			throw new Error(error.detail || `Failed to update note: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; revision_id: string };
	},

	/** Delete a note */
	async delete(workspaceId: string, noteId: string): Promise<void> {
		const res = await apiFetch(`/workspaces/${workspaceId}/notes/${noteId}`, {
			method: "DELETE",
		});
		if (!res.ok) {
			throw new Error(`Failed to delete note: ${res.statusText}`);
		}
	},

	/** Query notes with filters */
	async query(
		workspaceId: string,
		filter: Record<string, unknown>,
	): Promise<NoteRecord[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/query`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ filter }),
		});
		if (!res.ok) {
			throw new Error(`Failed to query notes: ${res.statusText}`);
		}
		return (await res.json()) as NoteRecord[];
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
