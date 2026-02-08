import { useParams } from "@solidjs/router";
import { createResource, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { spaceApi } from "~/lib/space-api";

export default function SpaceDashboardRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;

	const [space] = createResource(async () => {
		return await spaceApi.get(spaceId());
	});

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="dashboard">
			<div class="mx-auto max-w-4xl">
				<Show when={space.loading}>
					<p class="text-sm ui-muted">Loading space...</p>
				</Show>
				<Show when={space.error}>
					<p class="text-sm text-red-600">Failed to load space.</p>
				</Show>
				<Show when={space()}>{(ws) => <h1 class="ui-page-title text-4xl">{ws().name}</h1>}</Show>
			</div>
		</SpaceShell>
	);
}
