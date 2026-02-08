import { useParams } from "@solidjs/router";
import { createResource, Show } from "solid-js";
import { SpaceShell } from "~/components/SpaceShell";
import { SpaceSettings } from "~/components/SpaceSettings";
import { spaceApi } from "~/lib/space-api";
import type { SpacePatchPayload } from "~/lib/types";

export default function SpaceSettingsRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;

	const [space, { refetch }] = createResource(async () => {
		return await spaceApi.get(spaceId());
	});

	const handleSave = async (payload: SpacePatchPayload) => {
		await spaceApi.patch(spaceId(), payload);
		await refetch();
	};

	const handleTestConnection = async (config: { uri: string }) => {
		return await spaceApi.testConnection(spaceId(), {
			storage_config: { uri: config.uri },
		});
	};

	return (
		<SpaceShell spaceId={spaceId()}>
			<div class="mx-auto max-w-5xl">
				<h1 class="ui-page-title">Space Settings</h1>
				<p class="ui-page-subtitle mt-1">Space ID: {spaceId()}</p>

				<div class="mt-6">
					<Show when={space.loading}>
						<p class="text-sm ui-muted">Loading space...</p>
					</Show>
					<Show when={space.error}>
						<p class="text-sm ui-text-danger">Failed to load space.</p>
					</Show>
					<Show when={space()}>
						{(ws) => (
							<SpaceSettings
								space={ws()}
								onSave={handleSave}
								onTestConnection={handleTestConnection}
							/>
						)}
					</Show>
				</div>
			</div>
		</SpaceShell>
	);
}
