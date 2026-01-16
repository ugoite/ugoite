import type {
	Attachment,
	Note,
	NoteCreatePayload,
	NoteRecord,
	NoteRevision,
	NoteUpdatePayload,
	SearchResult,
	Workspace,
	WorkspaceLink,
	WorkspacePatchPayload,
	TestConnectionPayload,
	Class,
	ClassCreatePayload,
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

	/** Patch workspace metadata/settings */
	async patch(id: string, payload: WorkspacePatchPayload): Promise<Workspace> {
		const res = await apiFetch(`/workspaces/${id}`, {
			method: "PATCH",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to patch workspace: ${res.statusText}`);
		}
		return (await res.json()) as Workspace;
	},

	/** Test storage connection */
	async testConnection(id: string, payload: TestConnectionPayload): Promise<{ status: string }> {
		const res = await apiFetch(`/workspaces/${id}/test-connection`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to test connection: ${res.statusText}`);
		}
		return (await res.json()) as { status: string };
	},

	/** Query workspace index */
	async query(id: string, filter: Record<string, unknown>): Promise<NoteRecord[]> {
		const res = await apiFetch(`/workspaces/${id}/query`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ filter }),
		});
		if (!res.ok) {
			throw new Error(`Failed to query workspace: ${res.statusText}`);
		}
		return (await res.json()) as NoteRecord[];
	},
};

/**
 * Class API client
 */
export const classApi = {
	async listTypes(workspaceId: string): Promise<string[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/classes/types`);
		if (!res.ok) {
			throw new Error(`Failed to list class types: ${res.statusText}`);
		}
		return (await res.json()) as string[];
	},

	async list(workspaceId: string): Promise<Class[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/classes`);
		if (!res.ok) throw new Error(`Failed to list classes: ${res.statusText}`);
		return (await res.json()) as Class[];
	},
	async get(workspaceId: string, className: string): Promise<Class> {
		const res = await apiFetch(`/workspaces/${workspaceId}/classes/${className}`);
		if (!res.ok) throw new Error(`Failed to get class: ${res.statusText}`);
		return (await res.json()) as Class;
	},
	async create(workspaceId: string, payload: ClassCreatePayload): Promise<Class> {
		const res = await apiFetch(`/workspaces/${workspaceId}/classes`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) throw new Error(`Failed to create class: ${res.statusText}`);
		return (await res.json()) as Class;
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
		const res = await apiFetch(`/workspaces/${workspaceId}/notes/${noteId}`, {
			method: "DELETE",
		});
		if (!res.ok) {
			throw new Error(`Failed to delete note: ${res.statusText}`);
		}
	},

	/** Search notes by keyword */
	async search(workspaceId: string, query: string): Promise<SearchResult[]> {
		const params = new URLSearchParams({ q: query });
		const res = await apiFetch(`/workspaces/${workspaceId}/search?${params.toString()}`);
		if (!res.ok) {
			throw new Error(`Failed to search notes: ${res.statusText}`);
		}
		return (await res.json()) as SearchResult[];
	},

	/** Get note revision history */
	async history(workspaceId: string, noteId: string): Promise<{ revisions: NoteRevision[] }> {
		const res = await apiFetch(`/workspaces/${workspaceId}/notes/${noteId}/history`);
		if (!res.ok) {
			throw new Error(`Failed to get note history: ${res.statusText}`);
		}
		return (await res.json()) as { revisions: NoteRevision[] };
	},

	/** Get a specific note revision */
	async getRevision(workspaceId: string, noteId: string, revisionId: string): Promise<Note> {
		const res = await apiFetch(`/workspaces/${workspaceId}/notes/${noteId}/history/${revisionId}`);
		if (!res.ok) {
			throw new Error(`Failed to get note revision: ${res.statusText}`);
		}
		return (await res.json()) as Note;
	},

	/** Restore note to a previous revision */
	async restore(workspaceId: string, noteId: string, revisionId: string): Promise<Note> {
		const res = await apiFetch(`/workspaces/${workspaceId}/notes/${noteId}/restore`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ revision_id: revisionId }),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to restore note: ${res.statusText}`);
		}
		return (await res.json()) as Note;
	},
};

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

/** Links API client */
export const linksApi = {
	async create(
		workspaceId: string,
		payload: { source: string; target: string; kind: string },
	): Promise<WorkspaceLink> {
		const res = await apiFetch(`/workspaces/${workspaceId}/links`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create link: ${res.statusText}`);
		}
		return (await res.json()) as WorkspaceLink;
	},

	async list(workspaceId: string): Promise<WorkspaceLink[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/links`);
		if (!res.ok) {
			throw new Error(`Failed to list links: ${res.statusText}`);
		}
		return (await res.json()) as WorkspaceLink[];
	},

	async delete(workspaceId: string, linkId: string): Promise<{ status: string; id: string }> {
		const res = await apiFetch(`/workspaces/${workspaceId}/links/${linkId}`, { method: "DELETE" });
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to delete link: ${res.statusText}`);
		}
		return (await res.json()) as { status: string; id: string };
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
