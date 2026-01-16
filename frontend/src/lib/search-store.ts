import { createSignal } from "solid-js";
import type { NoteRecord, SearchResult } from "./types";
import { searchApi } from "./search-api";

export function createSearchStore(workspaceId: () => string) {
	const [results, setResults] = createSignal<SearchResult[]>([]);
	const [queryResults, setQueryResults] = createSignal<NoteRecord[]>([]);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	async function searchKeyword(query: string): Promise<SearchResult[]> {
		setLoading(true);
		setError(null);
		try {
			const data = await searchApi.keyword(workspaceId(), query);
			setResults(data);
			return data;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to search notes");
			throw e;
		} finally {
			setLoading(false);
		}
	}

	async function queryIndex(filter: Record<string, unknown>): Promise<NoteRecord[]> {
		setLoading(true);
		setError(null);
		try {
			const data = await searchApi.query(workspaceId(), filter);
			setQueryResults(data);
			return data;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to query notes");
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
