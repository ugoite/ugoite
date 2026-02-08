import { A, useNavigate } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { spaceApi } from "~/lib/space-api";

export default function SpacesIndexRoute() {
	const navigate = useNavigate();
	const [newSpaceName, setNewSpaceName] = createSignal("");
	const [createError, setCreateError] = createSignal<string | null>(null);
	const [isCreating, setIsCreating] = createSignal(false);

	const [spaces, { refetch }] = createResource(async () => {
		return await spaceApi.list();
	});

	const handleCreate = async () => {
		const name = newSpaceName().trim();
		if (!name) return;
		setIsCreating(true);
		setCreateError(null);
		try {
			const created = await spaceApi.create(name);
			await refetch();
			setNewSpaceName("");
			navigate(`/spaces/${created.id}/dashboard`);
		} catch (err) {
			setCreateError(err instanceof Error ? err.message : "Failed to create space");
		} finally {
			setIsCreating(false);
		}
	};

	return (
		<main class="mx-auto max-w-4xl ui-page ui-stack">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<h1 class="ui-page-title">Spaces</h1>
				<A href="/" class="ui-muted text-sm">
					Back to Home
				</A>
			</div>

			<section class="ui-card">
				<h2 class="text-lg font-semibold mb-2">Create Space</h2>
				<div class="flex flex-col gap-3 sm:flex-row sm:items-center">
					<input
						type="text"
						class="ui-input flex-1"
						placeholder="Space name"
						value={newSpaceName()}
						onInput={(e) => setNewSpaceName(e.currentTarget.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") handleCreate();
						}}
					/>
					<button
						type="button"
						class="ui-button ui-button-primary"
						disabled={isCreating()}
						onClick={handleCreate}
					>
						{isCreating() ? "Creating..." : "Create"}
					</button>
				</div>
				<Show when={createError()}>
					<p class="ui-alert ui-alert-error text-sm mt-2">{createError()}</p>
				</Show>
			</section>

			<section class="ui-card">
				<h2 class="text-lg font-semibold mb-3">Available Spaces</h2>
				<Show when={spaces.loading}>
					<p class="text-sm ui-muted">Loading spaces...</p>
				</Show>
				<Show when={spaces.error}>
					<p class="ui-alert ui-alert-error text-sm">Failed to load spaces.</p>
				</Show>
				<Show when={spaces() && spaces()?.length === 0}>
					<p class="text-sm ui-muted">No spaces yet. Create one above.</p>
				</Show>
				<ul class="ui-stack-sm">
					<For each={spaces() || []}>
						{(space) => (
							<li class="ui-card flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
								<div>
									<h3 class="font-medium">{space.name || space.id}</h3>
									<p class="text-xs ui-muted">ID: {space.id}</p>
								</div>
								<div class="flex flex-wrap gap-2">
									<A
										href={`/spaces/${space.id}/settings`}
										class="ui-button ui-button-secondary text-sm"
									>
										Settings
									</A>
									<A
										href={`/spaces/${space.id}/dashboard`}
										class="ui-button ui-button-primary text-sm"
									>
										Open Space
									</A>
								</div>
							</li>
						)}
					</For>
				</ul>
			</section>
		</main>
	);
}
