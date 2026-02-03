import { useParams } from "@solidjs/router";
import { createResource, Show } from "solid-js";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { WorkspaceSettings } from "~/components/WorkspaceSettings";
import { workspaceApi } from "~/lib/workspace-api";
import type { WorkspacePatchPayload } from "~/lib/types";

export default function WorkspaceSettingsRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;

	const [workspace, { refetch }] = createResource(async () => {
		return await workspaceApi.get(workspaceId());
	});

	const handleSave = async (payload: WorkspacePatchPayload) => {
		await workspaceApi.patch(workspaceId(), payload);
		await refetch();
	};

	const handleTestConnection = async (config: { uri: string }) => {
		return await workspaceApi.testConnection(workspaceId(), {
			storage_config: { uri: config.uri },
		});
	};

	return (
		<WorkspaceShell workspaceId={workspaceId()}>
			<div class="mx-auto max-w-5xl">
				<h1 class="text-2xl font-semibold text-slate-900">Workspace Settings</h1>
				<p class="text-sm text-slate-500 mt-1">Workspace ID: {workspaceId()}</p>

				<div class="mt-6">
					<Show when={workspace.loading}>
						<p class="text-sm text-slate-500">Loading workspace...</p>
					</Show>
					<Show when={workspace.error}>
						<p class="text-sm text-red-600">Failed to load workspace.</p>
					</Show>
					<Show when={workspace()}>
						{(ws) => (
							<WorkspaceSettings
								workspace={ws()}
								onSave={handleSave}
								onTestConnection={handleTestConnection}
							/>
						)}
					</Show>
				</div>
			</div>
		</WorkspaceShell>
	);
}
