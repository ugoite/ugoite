import { A, useParams } from "@solidjs/router";
import { createResource, createSignal, For, Show } from "solid-js";
import { linkApi } from "~/lib/link-api";

export default function WorkspaceLinksRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;
	const [source, setSource] = createSignal("");
	const [target, setTarget] = createSignal("");
	const [kind, setKind] = createSignal("related");
	const [actionError, setActionError] = createSignal<string | null>(null);
	const [isCreating, setIsCreating] = createSignal(false);

	const [links, { refetch }] = createResource(async () => {
		return await linkApi.list(workspaceId());
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
			await linkApi.create(workspaceId(), payload);
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
			await linkApi.delete(workspaceId(), linkId);
			await refetch();
		} catch (err) {
			setActionError(err instanceof Error ? err.message : "Failed to delete link");
		}
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-4xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="text-2xl font-bold text-gray-900">Links</h1>
					<A
						href={`/workspaces/${workspaceId()}/notes`}
						class="text-sm text-sky-700 hover:underline"
					>
						Back to Notes
					</A>
				</div>

				<section class="mb-6 bg-white border rounded-lg p-4">
					<h2 class="text-lg font-semibold mb-3">Create Link</h2>
					<div class="grid gap-3 sm:grid-cols-3">
						<input
							class="px-3 py-2 border rounded"
							placeholder="Source note ID"
							value={source()}
							onInput={(e) => setSource(e.currentTarget.value)}
						/>
						<input
							class="px-3 py-2 border rounded"
							placeholder="Target note ID"
							value={target()}
							onInput={(e) => setTarget(e.currentTarget.value)}
						/>
						<input
							class="px-3 py-2 border rounded"
							placeholder="Kind"
							value={kind()}
							onInput={(e) => setKind(e.currentTarget.value)}
						/>
					</div>
					<button
						type="button"
						class="mt-3 px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-50"
						onClick={handleCreate}
						disabled={isCreating()}
					>
						{isCreating() ? "Creating..." : "Create Link"}
					</button>
					<Show when={actionError()}>
						<p class="text-sm text-red-600 mt-2">{actionError()}</p>
					</Show>
				</section>

				<section class="bg-white border rounded-lg p-4">
					<h2 class="text-lg font-semibold mb-3">Workspace Links</h2>
					<Show when={links.loading}>
						<p class="text-sm text-gray-500">Loading links...</p>
					</Show>
					<Show when={links.error}>
						<p class="text-sm text-red-600">Failed to load links.</p>
					</Show>
					<Show when={links() && links()?.length === 0}>
						<p class="text-sm text-gray-500">No links yet.</p>
					</Show>
					<ul class="space-y-3">
						<For each={links() || []}>
							{(link) => (
								<li class="border rounded p-3 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
									<div>
										<p class="text-sm font-medium text-gray-800">{link.kind}</p>
										<p class="text-xs text-gray-500">
											{link.source} → {link.target}
										</p>
									</div>
									<div class="flex gap-2">
										<A
											href={`/workspaces/${workspaceId()}/links/${link.id}`}
											class="text-sm text-blue-600 hover:underline"
										>
											Details
										</A>
										<button
											type="button"
											class="text-sm text-red-600 hover:underline"
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
