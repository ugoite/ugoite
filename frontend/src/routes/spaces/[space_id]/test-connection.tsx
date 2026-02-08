import { A, useParams } from "@solidjs/router";
import { createResource, createSignal, Show } from "solid-js";
import { spaceApi } from "~/lib/space-api";

export default function SpaceTestConnectionRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;
	const [uri, setUri] = createSignal("");
	const [status, setStatus] = createSignal<string | null>(null);
	const [error, setError] = createSignal<string | null>(null);
	const [isTesting, setIsTesting] = createSignal(false);

	const [space] = createResource(async () => {
		const ws = await spaceApi.get(spaceId());
		setUri(ws.storage_config?.uri || "");
		return ws;
	});

	const handleTest = async () => {
		setError(null);
		setStatus(null);
		setIsTesting(true);
		try {
			const result = await spaceApi.testConnection(spaceId(), { uri: uri() });
			setStatus(result.status);
		} catch (err) {
			setError(err instanceof Error ? err.message : "Failed to test connection");
		} finally {
			setIsTesting(false);
		}
	};

	return (
		<main class="ui-shell ui-page">
			<div class="max-w-3xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<div>
						<h1 class="ui-page-title">Test Connection</h1>
						<p class="text-sm ui-muted">Space ID: {spaceId()}</p>
					</div>
					<A href={`/spaces/${spaceId()}`} class="text-sm">
						Back to Settings
					</A>
				</div>

				<Show when={space.loading}>
					<p class="text-sm ui-muted">Loading space...</p>
				</Show>
				<Show when={space.error}>
					<p class="ui-alert ui-alert-error text-sm">Failed to load space.</p>
				</Show>

				<div class="ui-card">
					<label class="ui-label text-sm mb-2" for="storage-uri">
						Storage URI
					</label>
					<input
						id="storage-uri"
						type="text"
						class="ui-input w-full"
						value={uri()}
						onInput={(e) => setUri(e.currentTarget.value)}
						placeholder="file:///local/path or s3://bucket/path"
					/>
					<button
						type="button"
						class="ui-button ui-button-primary mt-3"
						onClick={handleTest}
						disabled={isTesting() || !uri()}
					>
						{isTesting() ? "Testing..." : "Test Connection"}
					</button>
					<Show when={status()}>
						<p class="ui-alert ui-alert-success text-sm mt-2">Connection successful ({status()})</p>
					</Show>
					<Show when={error()}>
						<p class="ui-alert ui-alert-error text-sm mt-2">{error()}</p>
					</Show>
				</div>
			</div>
		</main>
	);
}
