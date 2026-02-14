import type {
	Entry,
	EntryCreatePayload,
	EntryRecord,
	EntryRevision,
	EntryUpdatePayload,
	Form,
} from "./types";
import { apiFetch } from "./api";
import { buildEntryMarkdownByMode } from "./entry-input";

/**
 * Entry API client
 */
export const entryApi = {
	/** List all entries in a space (uses index) */
	async list(spaceId: string): Promise<EntryRecord[]> {
		const res = await apiFetch(`/spaces/${encodeURIComponent(spaceId)}/entries`);
		if (!res.ok) {
			throw new Error(`Failed to list entries: ${res.statusText}`);
		}
		return (await res.json()) as EntryRecord[];
	},

	/** Get a single entry */
	async get(spaceId: string, entryId: string): Promise<Entry> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/entries/${encodeURIComponent(entryId)}`,
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
			throw new Error(`Failed to get entry: ${detail}`);
		}
		return (await res.json()) as Entry;
	},

	/** Create a new entry */
	async create(
		spaceId: string,
		payload: EntryCreatePayload,
	): Promise<{ id: string; revision_id: string }> {
		const res = await apiFetch(`/spaces/${encodeURIComponent(spaceId)}/entries`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create entry: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; revision_id: string };
	},

	/** Create a new entry from markdown editor input */
	async createFromMarkdown(
		spaceId: string,
		markdown: string,
		id?: string,
	): Promise<{ id: string; revision_id: string }> {
		return await this.create(spaceId, { id, content: markdown });
	},

	/** Create a new entry from webform field input */
	async createFromWebform(
		spaceId: string,
		formDef: Form,
		title: string,
		fieldValues: Record<string, string>,
		id?: string,
	): Promise<{ id: string; revision_id: string }> {
		const markdown = buildEntryMarkdownByMode(formDef, title, fieldValues, "webform");
		return await this.create(spaceId, { id, content: markdown });
	},

	/** Create a new entry from chat-style Q&A input */
	async createFromChat(
		spaceId: string,
		formDef: Form,
		title: string,
		answers: Record<string, string>,
		id?: string,
	): Promise<{ id: string; revision_id: string }> {
		const markdown = buildEntryMarkdownByMode(formDef, title, answers, "chat");
		return await this.create(spaceId, { id, content: markdown });
	},

	/** Update a entry (requires parent_revision_id for optimistic locking) */
	async update(
		spaceId: string,
		entryId: string,
		payload: EntryUpdatePayload,
	): Promise<{ id: string; revision_id: string }> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/entries/${encodeURIComponent(entryId)}`,
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
			throw new Error(error.detail || `Failed to update entry: ${res.statusText}`);
		}
		return (await res.json()) as { id: string; revision_id: string };
	},

	/** Delete a entry */
	async delete(spaceId: string, entryId: string): Promise<void> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/entries/${encodeURIComponent(entryId)}`,
			{
				method: "DELETE",
			},
		);
		if (!res.ok) {
			throw new Error(`Failed to delete entry: ${res.statusText}`);
		}
	},

	/** Get entry revision history */
	async history(spaceId: string, entryId: string): Promise<{ revisions: EntryRevision[] }> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/entries/${encodeURIComponent(entryId)}/history`,
		);
		if (!res.ok) {
			throw new Error(`Failed to get entry history: ${res.statusText}`);
		}
		return (await res.json()) as { revisions: EntryRevision[] };
	},

	/** Get a specific entry revision */
	async getRevision(spaceId: string, entryId: string, revisionId: string): Promise<Entry> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/entries/${encodeURIComponent(entryId)}/history/${encodeURIComponent(revisionId)}`,
		);
		if (!res.ok) {
			throw new Error(`Failed to get entry revision: ${res.statusText}`);
		}
		return (await res.json()) as Entry;
	},

	/** Restore entry to a previous revision */
	async restore(spaceId: string, entryId: string, revisionId: string): Promise<Entry> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/entries/${encodeURIComponent(entryId)}/restore`,
			{
				method: "POST",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify({ revision_id: revisionId }),
			},
		);
		if (!res.ok) {
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to restore entry: ${res.statusText}`);
		}
		return (await res.json()) as Entry;
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
