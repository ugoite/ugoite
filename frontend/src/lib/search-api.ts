import type { EntryRecord, SearchResult } from "./types";
import { apiFetch } from "./api";

export type EntrySummary = {
	id: string;
	title: string;
	form: string;
};

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

	/** List bounded entry summaries for row_reference pickers */
	async rowReferenceOptions(
		spaceId: string,
		targetForm: string,
		query: string,
		limit: number,
	): Promise<EntrySummary[]> {
		const params = new URLSearchParams({
			form: targetForm,
			limit: String(limit),
		});
		const trimmedQuery = query.trim();
		if (trimmedQuery) {
			params.set("q", trimmedQuery);
		}
		const res = await apiFetch(`/spaces/${spaceId}/entries/options?${params.toString()}`);
		if (!res.ok) {
			throw new Error(`Failed to load row_reference options: ${res.statusText}`);
		}
		return (await res.json()) as EntrySummary[];
	},
};
