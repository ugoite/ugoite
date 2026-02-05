import type { EntryRecord, SearchResult } from "./types";
import { apiFetch } from "./api";

/** Search & query API client */
export const searchApi = {
	/** Query space index */
	async query(spaceId: string, filter: Record<string, unknown>): Promise<EntryRecord[]> {
		const res = await apiFetch(`/spaces/${spaceId}/query`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ filter }),
		});
		if (!res.ok) {
			throw new Error(`Failed to query space: ${res.statusText}`);
		}
		return (await res.json()) as EntryRecord[];
	},

	/** Search entries by keyword */
	async keyword(spaceId: string, query: string): Promise<SearchResult[]> {
		const params = new URLSearchParams({ q: query });
		const res = await apiFetch(`/spaces/${spaceId}/search?${params.toString()}`);
		if (!res.ok) {
			throw new Error(`Failed to search entries: ${res.statusText}`);
		}
		return (await res.json()) as SearchResult[];
	},
};
