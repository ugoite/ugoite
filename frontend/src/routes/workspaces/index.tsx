import { A, useNavigate } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { workspaceApi } from "~/lib/workspace-api";

export default function WorkspacesIndexRoute() {
	const navigate = useNavigate();
	const [newWorkspaceName, setNewWorkspaceName] = createSignal("");
	const [createError, setCreateError] = createSignal<string | null>(null);
	const [isCreating, setIsCreating] = createSignal(false);

	const [workspaces, { refetch }] = createResource(async () => {
		return await workspaceApi.list();
	});

	const handleCreate = async () => {
		const name = newWorkspaceName().trim();
		if (!name) return;
		setIsCreating(true);
		setCreateError(null);
		try {
			const created = await workspaceApi.create(name);
			await refetch();
			setNewWorkspaceName("");
			navigate(`/workspaces/${created.id}/notes`);
		} catch (err) {
			setCreateError(err instanceof Error ? err.message : "Failed to create workspace");
		} finally {
			setIsCreating(false);
		}
	};

	return (
		<main class="mx-auto max-w-4xl p-6">
			<div class="flex items-center justify-between mb-6">
				<h1 class="text-2xl font-bold text-gray-900">Workspaces</h1>
				<A href="/" class="text-sm text-sky-700 hover:text-sky-800 hover:underline">
					Back to Home
				</A>
			</div>

			<section class="mb-8">
				<h2 class="text-lg font-semibold text-gray-800 mb-2">Create Workspace</h2>
				<div class="flex flex-col gap-3 sm:flex-row sm:items-center">
					<input
						type="text"
						class="flex-1 px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
						placeholder="Workspace name"
						value={newWorkspaceName()}
						onInput={(e) => setNewWorkspaceName(e.currentTarget.value)}
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
				<h2 class="text-lg font-semibold text-gray-800 mb-3">Available Workspaces</h2>
				<Show when={workspaces.loading}>
					<p class="text-sm text-gray-500">Loading workspaces...</p>
				</Show>
				<Show when={workspaces.error}>
					<p class="text-sm text-red-600">Failed to load workspaces.</p>
				</Show>
				<Show when={workspaces() && workspaces()?.length === 0}>
					<p class="text-sm text-gray-500">No workspaces yet. Create one above.</p>
				</Show>
				<ul class="space-y-3">
					<For each={workspaces() || []}>
						{(workspace) => (
							<li class="border rounded-lg p-4 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
								<div>
									<h3 class="font-medium text-gray-900">{workspace.name || workspace.id}</h3>
									<p class="text-xs text-gray-500">ID: {workspace.id}</p>
								</div>
								<div class="flex gap-2">
									<A
										href={`/workspaces/${workspace.id}`}
										class="px-3 py-1 text-sm border border-gray-300 rounded hover:bg-gray-50"
									>
										Settings
									</A>
									<A
										href={`/workspaces/${workspace.id}/notes`}
										class="px-3 py-1 text-sm bg-blue-600 text-white rounded hover:bg-blue-700"
									>
										Open Notes
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
