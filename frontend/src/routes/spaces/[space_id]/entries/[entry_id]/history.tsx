import { A, useParams } from "@solidjs/router";
import { createResource, For, Show } from "solid-js";
import { entryApi } from "~/lib/entry-api";

export default function SpaceEntryHistoryRoute() {
	const params = useParams<{ space_id: string; entry_id: string }>();
	const spaceId = () => params.space_id;
	const entryId = () => params.entry_id;
	const encodedEntryId = () => encodeURIComponent(entryId());

	const [history] = createResource(async () => {
		return await entryApi.history(spaceId(), entryId());
	});

	return (
		<main class="ui-page">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="ui-page-title">Entry History</h1>
					<p class="text-sm ui-muted">Entry ID: {entryId()}</p>
				</div>
				<A href={`/spaces/${spaceId()}/entries/${encodedEntryId()}`} class="text-sm ui-link">
					Back to Entry
				</A>
			</div>

			<Show when={history.loading}>
				<p class="text-sm ui-muted">Loading history...</p>
			</Show>
			<Show when={history.error}>
				<p class="text-sm ui-text-danger">Failed to load history.</p>
			</Show>
			<Show when={history()}>
				{(data) => (
					<ul class="space-y-3">
						<For each={data().revisions}>
							{(revision) => (
								<li class="ui-card flex items-center justify-between">
									<div>
										<p class="text-sm font-medium">Revision: {revision.revision_id}</p>
										<p class="text-xs ui-muted">{revision.created_at}</p>
									</div>
									<A
										href={`/spaces/${spaceId()}/entries/${encodedEntryId()}/history/${encodeURIComponent(revision.revision_id)}`}
										class="text-sm ui-link"
									>
										View Revision
									</A>
								</li>
							)}
						</For>
					</ul>
				)}
			</Show>
		</main>
	);
}
