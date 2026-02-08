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
		<main class="ui-page">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="ui-page-title">Entry Revision</h1>
					<p class="text-sm ui-muted">Revision ID: {revisionId()}</p>
				</div>
				<A
					href={`/spaces/${spaceId()}/entries/${encodeURIComponent(entryId())}/history`}
					class="text-sm ui-link"
				>
					Back to History
				</A>
			</div>

			<Show when={revision.loading}>
				<p class="text-sm ui-muted">Loading revision...</p>
			</Show>
			<Show when={revision.error}>
				<p class="text-sm ui-text-danger">Failed to load revision.</p>
			</Show>
			<Show when={revision()}>
				{(entry) => (
					<div class="ui-card">
						<h2 class="text-lg font-semibold mb-2">{entry().title}</h2>
						<pre class="text-sm whitespace-pre-wrap">{entry().content}</pre>
					</div>
				)}
			</Show>
		</main>
	);
}
