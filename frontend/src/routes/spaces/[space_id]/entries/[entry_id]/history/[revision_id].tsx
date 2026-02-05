import { A, useParams } from "@solidjs/router";
import { createResource, Show } from "solid-js";
import { entryApi } from "~/lib/entry-api";

export default function SpaceEntryRevisionRoute() {
	const params = useParams<{ space_id: string; entry_id: string; revision_id: string }>();
	const spaceId = () => params.space_id;
	const entryId = () => params.entry_id;
	const revisionId = () => params.revision_id;

	const [revision] = createResource(async () => {
		return await entryApi.getRevision(spaceId(), entryId(), revisionId());
	});

	return (
		<main class="flex-1 overflow-auto p-6">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="text-xl font-semibold text-gray-900">Entry Revision</h1>
					<p class="text-sm text-gray-500">Revision ID: {revisionId()}</p>
				</div>
				<A
					href={`/spaces/${spaceId()}/entries/${encodeURIComponent(entryId())}/history`}
					class="text-sm text-sky-700 hover:underline"
				>
					Back to History
				</A>
			</div>

			<Show when={revision.loading}>
				<p class="text-sm text-gray-500">Loading revision...</p>
			</Show>
			<Show when={revision.error}>
				<p class="text-sm text-red-600">Failed to load revision.</p>
			</Show>
			<Show when={revision()}>
				{(entry) => (
					<div class="bg-white border rounded-lg p-4">
						<h2 class="text-lg font-semibold mb-2">{entry().title}</h2>
						<pre class="text-sm whitespace-pre-wrap text-gray-700">{entry().content}</pre>
					</div>
				)}
			</Show>
		</main>
	);
}
