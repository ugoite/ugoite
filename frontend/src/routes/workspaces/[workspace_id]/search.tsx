import { A, useParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { searchApi } from "~/lib/search-api";
import type { SearchResult } from "~/lib/types";

export default function WorkspaceSearchRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;
	const [query, setQuery] = createSignal("");
	const [results, setResults] = createSignal<SearchResult[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	const handleSearch = async () => {
		const q = query().trim();
		if (!q) return;
		setIsSearching(true);
		setError(null);
		try {
			const data = await searchApi.keyword(workspaceId(), q);
			setResults(data);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Search failed");
		} finally {
			setIsSearching(false);
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-4xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="text-2xl font-bold text-gray-900">Search Notes</h1>
					<A
						href={`/workspaces/${workspaceId()}/notes`}
						class="text-sm text-sky-700 hover:underline"
					>
						Back to Notes
					</A>
				</div>

				<div class="bg-white border rounded-lg p-4 mb-6">
					<div class="flex flex-col sm:flex-row gap-3">
						<input
							type="text"
							class="flex-1 px-3 py-2 border rounded"
							placeholder="Search query"
							value={query()}
							onInput={(e) => setQuery(e.currentTarget.value)}
							onKeyDown={(e) => {
								if (e.key === "Enter") handleSearch();
							}}
						/>
						<button
							type="button"
							class="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50"
							onClick={handleSearch}
							disabled={isSearching()}
						>
							{isSearching() ? "Searching..." : "Search"}
						</button>
					</div>
					<Show when={error()}>
						<p class="text-sm text-red-600 mt-2">{error()}</p>
					</Show>
				</div>

				<div class="bg-white border rounded-lg p-4">
					<h2 class="text-lg font-semibold mb-3">Results</h2>
					<Show when={results().length === 0}>
						<p class="text-sm text-gray-500">No results yet.</p>
					</Show>
					<ul class="space-y-2">
						<For each={results()}>
							{(result) => (
								<li class="border rounded p-3">
									<A
										href={`/workspaces/${workspaceId()}/notes/${result.id}`}
										class="text-sm font-medium text-blue-600 hover:underline"
									>
										{result.title || result.id}
									</A>
									<p class="text-xs text-gray-500">Updated: {result.updated_at}</p>
								</li>
							)}
						</For>
					</ul>
				</div>
			</div>
		</main>
	);
}
