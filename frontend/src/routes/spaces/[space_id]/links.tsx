import { A, useParams } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { linkApi } from "~/lib/link-api";

export default function SpaceLinksRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;
	const [source, setSource] = createSignal("");
	const [target, setTarget] = createSignal("");
	const [kind, setKind] = createSignal("related");
	const [actionError, setActionError] = createSignal<string | null>(null);
	const [isCreating, setIsCreating] = createSignal(false);

	const [links, { refetch }] = createResource(async () => {
		return await linkApi.list(spaceId());
	});

	const handleCreate = async () => {
		setActionError(null);
		setIsCreating(true);
		try {
			const payload = { source: source().trim(), target: target().trim(), kind: kind().trim() };
			if (!payload.source || !payload.target || !payload.kind) {
				setActionError("Source, target, and kind are required.");
				return;
			}
			await linkApi.create(spaceId(), payload);
			await refetch();
			setSource("");
			setTarget("");
		} catch (err) {
			setActionError(err instanceof Error ? err.message : "Failed to create link");
		} finally {
			setIsCreating(false);
		}
	};

	const handleDelete = async (linkId: string) => {
		setActionError(null);
		try {
			await linkApi.delete(spaceId(), linkId);
			await refetch();
		} catch (err) {
			setActionError(err instanceof Error ? err.message : "Failed to delete link");
		}
	};

	return (
		<main class="ui-shell ui-page">
			<div class="max-w-4xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="ui-page-title">Links</h1>
					<A href={`/spaces/${spaceId()}/entries`} class="text-sm">
						Back to Entries
					</A>
				</div>

				<section class="mb-6 ui-card">
					<h2 class="text-lg font-semibold mb-3">Create Link</h2>
					<div class="grid gap-3 sm:grid-cols-3">
						<input
							class="ui-input"
							placeholder="Source entry ID"
							value={source()}
							onInput={(e) => setSource(e.currentTarget.value)}
						/>
						<input
							class="ui-input"
							placeholder="Target entry ID"
							value={target()}
							onInput={(e) => setTarget(e.currentTarget.value)}
						/>
						<input
							class="ui-input"
							placeholder="Kind"
							value={kind()}
							onInput={(e) => setKind(e.currentTarget.value)}
						/>
					</div>
					<button
						type="button"
						class="ui-button ui-button-primary mt-3"
						onClick={handleCreate}
						disabled={isCreating()}
					>
						{isCreating() ? "Creating..." : "Create Link"}
					</button>
					<Show when={actionError()}>
						<p class="ui-alert ui-alert-error text-sm mt-2">{actionError()}</p>
					</Show>
				</section>

				<section class="ui-card">
					<h2 class="text-lg font-semibold mb-3">Space Links</h2>
					<Show when={links.loading}>
						<p class="text-sm ui-muted">Loading links...</p>
					</Show>
					<Show when={links.error}>
						<p class="ui-alert ui-alert-error text-sm">Failed to load links.</p>
					</Show>
					<Show when={links() && links()?.length === 0}>
						<p class="text-sm ui-muted">No links yet.</p>
					</Show>
					<ul class="space-y-3">
						<For each={links() || []}>
							{(link) => (
								<li class="ui-card ui-card-hover flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
									<div>
										<p class="text-sm font-medium">{link.kind}</p>
										<p class="text-xs ui-muted">
											{link.source} → {link.target}
										</p>
									</div>
									<div class="flex gap-2">
										<A
											href={`/spaces/${spaceId()}/links/${encodeURIComponent(link.id)}`}
											class="ui-button ui-button-secondary text-sm"
										>
											Details
										</A>
										<button
											type="button"
											class="ui-button ui-button-danger text-sm"
											onClick={() => handleDelete(link.id)}
										>
											Delete
										</button>
									</div>
								</li>
							)}
						</For>
					</ul>
				</section>
			</div>
		</main>
	);
}
