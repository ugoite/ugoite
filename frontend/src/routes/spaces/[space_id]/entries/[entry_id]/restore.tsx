import { A, useNavigate, useParams } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { entryApi } from "~/lib/entry-api";

export default function SpaceEntryRestoreRoute() {
	const navigate = useNavigate();
	const params = useParams<{ space_id: string; entry_id: string }>();
	const spaceId = () => params.space_id;
	const entryId = () => params.entry_id;
	const encodedEntryId = () => encodeURIComponent(entryId());
	const [selectedRevision, setSelectedRevision] = createSignal<string | null>(null);
	const [restoreError, setRestoreError] = createSignal<string | null>(null);
	const [isRestoring, setIsRestoring] = createSignal(false);

	const [history] = createResource(async () => {
		return await entryApi.history(spaceId(), entryId());
	});

	const handleRestore = async () => {
		const revisionId = selectedRevision();
		if (!revisionId) return;
		setIsRestoring(true);
		setRestoreError(null);
		try {
			await entryApi.restore(spaceId(), entryId(), revisionId);
			navigate(`/spaces/${spaceId()}/entries/${encodedEntryId()}`);
		} catch (err) {
			setRestoreError(err instanceof Error ? err.message : "Failed to restore entry");
		} finally {
			setIsRestoring(false);
		}
	};

	return (
		<main class="ui-page">
			<div class="flex items-center justify-between mb-4">
				<div>
					<h1 class="ui-page-title">Restore Entry</h1>
					<p class="text-sm ui-muted">Select a revision to restore.</p>
				</div>
				<A href={`/spaces/${spaceId()}/entries/${encodedEntryId()}`} class="text-sm ui-link">
					Back to Entry
				</A>
			</div>

			<Show when={history.loading}>
				<p class="text-sm ui-muted">Loading revisions...</p>
			</Show>
			<Show when={history.error}>
				<p class="text-sm ui-text-danger">Failed to load history.</p>
			</Show>
			<Show when={history()}>
				{(data) => (
					<div class="ui-stack-sm">
						<ul class="space-y-2">
							<For each={data().revisions}>
								{(revision) => (
									<li class="flex items-center gap-2">
										<input
											type="radio"
											name="revision"
											value={revision.revision_id}
											checked={selectedRevision() === revision.revision_id}
											onChange={() => setSelectedRevision(revision.revision_id)}
										/>
										<span class="text-sm">{revision.revision_id}</span>
										<span class="text-xs ui-muted">{revision.created_at}</span>
									</li>
								)}
							</For>
						</ul>
						<button
							type="button"
							class="ui-button ui-button-primary"
							onClick={handleRestore}
							disabled={!selectedRevision() || isRestoring()}
						>
							{isRestoring() ? "Restoring..." : "Restore Selected Revision"}
						</button>
						<Show when={restoreError()}>
							<p class="text-sm ui-text-danger">{restoreError()}</p>
						</Show>
					</div>
				)}
			</Show>
		</main>
	);
}
