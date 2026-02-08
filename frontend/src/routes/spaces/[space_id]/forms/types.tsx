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
		<main class="ui-page">
			<div class="flex items-center justify-between mb-4">
				<h1 class="ui-page-title">Form Field Types</h1>
				<A href={`/spaces/${spaceId()}/forms`} class="text-sm ui-link">
					Back to Forms
				</A>
			</div>

			<Show when={types.loading}>
				<p class="text-sm ui-muted">Loading types...</p>
			</Show>
			<Show when={types.error}>
				<p class="text-sm ui-text-danger">Failed to load form types.</p>
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
