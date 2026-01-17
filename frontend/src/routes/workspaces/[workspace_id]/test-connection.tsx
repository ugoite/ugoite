import { A, useParams } from "@solidjs/router";
import { createResource, createSignal, Show } from "solid-js";
import { workspaceApi } from "~/lib/workspace-api";

export default function WorkspaceTestConnectionRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;
	const [uri, setUri] = createSignal("");
	const [status, setStatus] = createSignal<string | null>(null);
	const [error, setError] = createSignal<string | null>(null);
	const [isTesting, setIsTesting] = createSignal(false);

	const [workspace] = createResource(async () => {
		const ws = await workspaceApi.get(workspaceId());
		setUri(ws.storage_config?.uri || "");
		return ws;
	});

	const handleTest = async () => {
		setError(null);
		setStatus(null);
		setIsTesting(true);
		try {
			const result = await workspaceApi.testConnection(workspaceId(), { uri: uri() });
			setStatus(result.status);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Failed to test connection");
		} finally {
			setIsTesting(false);
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-3xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<div>
						<h1 class="text-2xl font-bold text-gray-900">Test Connection</h1>
						<p class="text-sm text-gray-500">Workspace ID: {workspaceId()}</p>
					</div>
					<A href={`/workspaces/${workspaceId()}`} class="text-sm text-sky-700 hover:underline">
						Back to Settings
					</A>
				</div>

				<Show when={workspace.loading}>
					<p class="text-sm text-gray-500">Loading workspace...</p>
				</Show>
				<Show when={workspace.error}>
					<p class="text-sm text-red-600">Failed to load workspace.</p>
				</Show>

				<div class="bg-white border rounded-lg p-4">
					<label class="block text-sm font-medium text-gray-700 mb-2" for="storage-uri">
						Storage URI
					</label>
					<input
						id="storage-uri"
						type="text"
						class="w-full px-3 py-2 border rounded"
						value={uri()}
						onInput={(e) => setUri(e.currentTarget.value)}
						placeholder="file:///local/path or s3://bucket/path"
					/>
					<button
						type="button"
						class="mt-3 px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50"
						onClick={handleTest}
						disabled={isTesting() || !uri()}
					>
						{isTesting() ? "Testing..." : "Test Connection"}
					</button>
					<Show when={status()}>
						<p class="text-sm text-green-600 mt-2">Connection successful ({status()})</p>
					</Show>
					<Show when={error()}>
						<p class="text-sm text-red-600 mt-2">{error()}</p>
					</Show>
				</div>
			</div>
		</main>
	);
}
