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
		<main class="flex-1 overflow-auto p-6">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="text-xl font-semibold text-gray-900">Entry History</h1>
					<p class="text-sm text-gray-500">Entry ID: {entryId()}</p>
				</div>
				<A
					href={`/spaces/${spaceId()}/entries/${encodedEntryId()}`}
					class="text-sm text-sky-700 hover:underline"
				>
					Back to Entry
				</A>
			</div>

			<Show when={history.loading}>
				<p class="text-sm text-gray-500">Loading history...</p>
			</Show>
			<Show when={history.error}>
				<p class="text-sm text-red-600">Failed to load history.</p>
			</Show>
			<Show when={history()}>
				{(data) => (
					<ul class="space-y-3">
						<For each={data().revisions}>
							{(revision) => (
								<li class="border rounded-lg p-4 flex items-center justify-between">
									<div>
										<p class="text-sm font-medium text-gray-800">
											Revision: {revision.revision_id}
										</p>
										<p class="text-xs text-gray-500">{revision.created_at}</p>
									</div>
									<A
										href={`/spaces/${spaceId()}/entries/${encodedEntryId()}/history/${encodeURIComponent(revision.revision_id)}`}
										class="text-sm text-blue-600 hover:underline"
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
