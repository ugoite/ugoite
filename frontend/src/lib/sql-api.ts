import { apiFetch } from "./api";
import { normalizeTimestamp } from "./date-format";
import type { SqlCreatePayload, SqlEntry, SqlUpdatePayload } from "./types";

type SqlMutationResponse = {
	id: string;
	revisionId: string;
};

const normalizeSqlEntry = (entry: SqlEntry): SqlEntry => ({
	...entry,
	["created_at"]: normalizeTimestamp(entry.created_at),
	["updated_at"]: normalizeTimestamp(entry.updated_at),
});

export const sqlApi = {
	async list(spaceId: string): Promise<SqlEntry[]> {
		const res = await apiFetch(`/spaces/${encodeURIComponent(spaceId)}/sql`);
		if (!res.ok) {
			throw new Error(`Failed to list saved SQL: ${res.statusText}`);
		}
		return ((await res.json()) as SqlEntry[]).map(normalizeSqlEntry);
	},

	async get(spaceId: string, sqlId: string): Promise<SqlEntry> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/sql/${encodeURIComponent(sqlId)}`,
		);
		if (!res.ok) {
			throw new Error(`Failed to get saved SQL: ${res.statusText}`);
		}
		return normalizeSqlEntry((await res.json()) as SqlEntry);
	},

	async create(spaceId: string, payload: SqlCreatePayload): Promise<SqlMutationResponse> {
		const res = await apiFetch(`/spaces/${encodeURIComponent(spaceId)}/sql`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(payload),
		});
		if (!res.ok) {
			/* v8 ignore start */
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to create saved SQL: ${res.statusText}`);
			/* v8 ignore stop */
		}
		const data = (await res.json()) as Record<string, string>;
		return { id: data.id, revisionId: data.revision_id };
	},

	async update(
		spaceId: string,
		sqlId: string,
		payload: SqlUpdatePayload,
	): Promise<SqlMutationResponse> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/sql/${encodeURIComponent(sqlId)}`,
			{
				method: "PUT",
				headers: { "Content-Type": "application/json" },
				body: JSON.stringify(payload),
			},
		);
		if (!res.ok) {
			/* v8 ignore start */
			const error = (await res.json()) as { detail?: string };
			throw new Error(error.detail || `Failed to update saved SQL: ${res.statusText}`);
			/* v8 ignore stop */
		}
		const data = (await res.json()) as Record<string, string>;
		return { id: data.id, revisionId: data.revision_id };
	},

	async delete(spaceId: string, sqlId: string): Promise<void> {
		const res = await apiFetch(
			`/spaces/${encodeURIComponent(spaceId)}/sql/${encodeURIComponent(sqlId)}`,
			{
				method: "DELETE",
			},
		);
		if (!res.ok) {
			throw new Error(`Failed to delete saved SQL: ${res.statusText}`);
		}
	},
};
