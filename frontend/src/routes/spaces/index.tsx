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
		<main class="mx-auto max-w-4xl p-6">
			<div class="flex items-center justify-between mb-6">
				<h1 class="text-2xl font-bold text-gray-900">Spaces</h1>
				<A href="/" class="text-sm text-sky-700 hover:text-sky-800 hover:underline">
					Back to Home
				</A>
			</div>

			<section class="mb-8">
				<h2 class="text-lg font-semibold text-gray-800 mb-2">Create Space</h2>
				<div class="flex flex-col gap-3 sm:flex-row sm:items-center">
					<input
						type="text"
						class="flex-1 px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
						placeholder="Space name"
						value={newSpaceName()}
						onInput={(e) => setNewSpaceName(e.currentTarget.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") handleCreate();
						}}
					/>
					<button
						type="button"
						class="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
						disabled={isCreating()}
						onClick={handleCreate}
					>
						{isCreating() ? "Creating..." : "Create"}
					</button>
				</div>
				<Show when={createError()}>
					<p class="text-sm text-red-600 mt-2">{createError()}</p>
				</Show>
			</section>

			<section>
				<h2 class="text-lg font-semibold text-gray-800 mb-3">Available Spaces</h2>
				<Show when={spaces.loading}>
					<p class="text-sm text-gray-500">Loading spaces...</p>
				</Show>
				<Show when={spaces.error}>
					<p class="text-sm text-red-600">Failed to load spaces.</p>
				</Show>
				<Show when={spaces() && spaces()?.length === 0}>
					<p class="text-sm text-gray-500">No spaces yet. Create one above.</p>
				</Show>
				<ul class="space-y-3">
					<For each={spaces() || []}>
						{(space) => (
							<li class="border rounded-lg p-4 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
								<div>
									<h3 class="font-medium text-gray-900">{space.name || space.id}</h3>
									<p class="text-xs text-gray-500">ID: {space.id}</p>
								</div>
								<div class="flex gap-2">
									<A
										href={`/spaces/${space.id}/settings`}
										class="px-3 py-1 text-sm border border-gray-300 rounded hover:bg-gray-50"
									>
										Settings
									</A>
									<A
										href={`/spaces/${space.id}/dashboard`}
										class="px-3 py-1 text-sm bg-blue-600 text-white rounded hover:bg-blue-700"
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
