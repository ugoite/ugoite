import { A, useParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { searchApi } from "~/lib/search-api";
import type { NoteRecord } from "~/lib/types";

export default function WorkspaceQueryRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;
	const [filterInput, setFilterInput] = createSignal("{}\n");
	const [results, setResults] = createSignal<NoteRecord[]>([]);
	const [isRunning, setIsRunning] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	const handleRun = async () => {
		setError(null);
		setIsRunning(true);
		try {
			const parsed = JSON.parse(filterInput());
			const data = await searchApi.query(workspaceId(), parsed as Record<string, unknown>);
			setResults(data);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Query failed");
		} finally {
			setIsRunning(false);
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-4xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="text-2xl font-bold text-gray-900">Query Notes</h1>
					<A
						href={`/workspaces/${workspaceId()}/notes`}
						class="text-sm text-sky-700 hover:underline"
					>
						Back to Notes
					</A>
				</div>

				<div class="bg-white border rounded-lg p-4 mb-6">
					<label class="block text-sm font-medium text-gray-700 mb-2" for="query-filter">
						Filter (JSON)
					</label>
					<textarea
						id="query-filter"
						class="w-full h-32 p-3 border rounded font-mono text-sm"
						value={filterInput()}
						onInput={(e) => setFilterInput(e.currentTarget.value)}
					/>
					<button
						type="button"
						class="mt-3 px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50"
						onClick={handleRun}
						disabled={isRunning()}
					>
						{isRunning() ? "Running..." : "Run Query"}
					</button>
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
