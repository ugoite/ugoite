import { A, useParams } from "@solidjs/router";
import { createResource, For, Show } from "solid-js";
import { formApi } from "~/lib/form-api";

export default function SpaceFormTypesRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;

	const [types] = createResource(async () => {
		return await formApi.listTypes(spaceId());
	});

	return (
		<main class="flex-1 overflow-auto p-6">
			<div class="flex items-center justify-between mb-4">
				<h1 class="text-xl font-semibold text-gray-900">Form Field Types</h1>
				<A href={`/spaces/${spaceId()}/forms`} class="text-sm text-sky-700 hover:underline">
					Back to Forms
				</A>
			</div>

			<Show when={types.loading}>
				<p class="text-sm text-gray-500">Loading types...</p>
			</Show>
			<Show when={types.error}>
				<p class="text-sm text-red-600">Failed to load form types.</p>
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
