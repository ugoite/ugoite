import type { NoteRecord, SearchResult } from "./types";
import { apiFetch } from "./api";

/** Search & query API client */
export const searchApi = {
	/** Query workspace index */
	async query(workspaceId: string, filter: Record<string, unknown>): Promise<NoteRecord[]> {
		const res = await apiFetch(`/workspaces/${workspaceId}/query`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ filter }),
		});
		if (!res.ok) {
			throw new Error(`Failed to query workspace: ${res.statusText}`);
		}
		return (await res.json()) as NoteRecord[];
	},

	/** Query workspace index via IEapp SQL */
	async querySql(workspaceId: string, sql: string): Promise<NoteRecord[]> {
		return searchApi.query(workspaceId, { $sql: sql });
	},

	/** Search notes by keyword */
	async keyword(workspaceId: string, query: string): Promise<SearchResult[]> {
		const params = new URLSearchParams({ q: query });
		const res = await apiFetch(`/workspaces/${workspaceId}/search?${params.toString()}`);
		if (!res.ok) {
			throw new Error(`Failed to search notes: ${res.statusText}`);
		}
		return (await res.json()) as SearchResult[];
	},
};
