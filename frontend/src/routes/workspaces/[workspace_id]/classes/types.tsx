import { A, useParams } from "@solidjs/router";
import { createResource, For, Show } from "solid-js";
import { classApi } from "~/lib/class-api";

export default function WorkspaceClassTypesRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;

	const [types] = createResource(async () => {
		return await classApi.listTypes(workspaceId());
	});

	return (
		<main class="flex-1 overflow-auto p-6">
			<div class="flex items-center justify-between mb-4">
				<h1 class="text-xl font-semibold text-gray-900">Class Field Types</h1>
				<A
					href={`/workspaces/${workspaceId()}/classes`}
					class="text-sm text-sky-700 hover:underline"
				>
					Back to Classes
				</A>
			</div>

			<Show when={types.loading}>
				<p class="text-sm text-gray-500">Loading types...</p>
			</Show>
			<Show when={types.error}>
				<p class="text-sm text-red-600">Failed to load class types.</p>
			</Show>
			<Show when={types()}>
				{(list) => (
					<ul class="space-y-2">
						<For each={list()}>{(item) => <li class="text-sm">{item}</li>}</For>
					</ul>
				)}
			</Show>
		</main>
	);
}
