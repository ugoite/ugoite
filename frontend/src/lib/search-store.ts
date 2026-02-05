import { createSignal } from "solid-js";
import type { EntryRecord, SearchResult } from "./types";
import { searchApi } from "./search-api";

export function createSearchStore(spaceId: () => string) {
	const [results, setResults] = createSignal<SearchResult[]>([]);
	const [queryResults, setQueryResults] = createSignal<EntryRecord[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function searchKeyword(query: string): Promise<SearchResult[]> {
		setLoading(true);
		setError(null);
		try {
			const data = await searchApi.keyword(spaceId(), query);
			setResults(data);
			return data;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to search entries");
			throw e;
		} finally {
			setLoading(false);
		}
	}

	async function queryIndex(filter: Record<string, unknown>): Promise<EntryRecord[]> {
		setLoading(true);
		setError(null);
		try {
			const data = await searchApi.query(spaceId(), filter);
			setQueryResults(data);
			return data;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to query entries");
			throw e;
		} finally {
			setLoading(false);
		}
	}

	return {
		results,
		queryResults,
		loading,
		error,
		searchKeyword,
		queryIndex,
	};
}

export type SearchStore = ReturnType<typeof createSearchStore>;
