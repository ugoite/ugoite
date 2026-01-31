import { A, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createSignal, For, Show, onMount } from "solid-js";
import { SqlQueryEditor } from "~/components";
import { classApi } from "~/lib/class-api";
import { searchApi } from "~/lib/search-api";
import { buildSqlSchema } from "~/lib/sql";
import type { Class, NoteRecord } from "~/lib/types";

export default function WorkspaceQueryRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;
	const [sqlInput, setSqlInput] = createSignal("SELECT * FROM notes LIMIT 50");
	const [classes, setClasses] = createSignal<Class[]>([]);
	const [results, setResults] = createSignal<NoteRecord[]>([]);
	const [isRunning, setIsRunning] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);
	const [diagnostics, setDiagnostics] = createSignal<Diagnostic[]>([]);

	onMount(async () => {
		try {
			const data = await classApi.list(workspaceId());
			setClasses(data);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Failed to load classes");
		}
	});

	const handleRun = async () => {
		setError(null);
		setIsRunning(true);
		try {
			const data = await searchApi.querySql(workspaceId(), sqlInput());
			setResults(data);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Query failed");
		} finally {
			setIsRunning(false);
		}
	};

	const schema = () => buildSqlSchema(classes());

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
						IEapp SQL
					</label>
					<SqlQueryEditor
						id="query-filter"
						value={sqlInput()}
						onChange={setSqlInput}
						schema={schema()}
						onDiagnostics={setDiagnostics}
						disabled={isRunning()}
					/>
					<button
						type="button"
						class="mt-3 px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50"
						onClick={handleRun}
						disabled={isRunning()}
					>
						{isRunning() ? "Running..." : "Run Query"}
					</button>
					<Show when={diagnostics().length > 0}>
						<ul class="mt-3 text-sm text-amber-700 space-y-1">
							<For each={diagnostics()}>{(diag) => <li>{diag.message}</li>}</For>
						</ul>
					</Show>
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
										href={`/workspaces/${workspaceId()}/notes/${encodeURIComponent(result.id)}`}
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
