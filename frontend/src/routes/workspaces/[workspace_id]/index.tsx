import { A, useParams } from "@solidjs/router";
import { createResource, Show } from "solid-js";
import { WorkspaceSettings } from "~/components/WorkspaceSettings";
import { workspaceApi } from "~/lib/workspace-api";
import type { WorkspacePatchPayload } from "~/lib/types";

export default function WorkspaceDetailRoute() {
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
		return await workspaceApi.testConnection(workspaceId(), { uri: config.uri });
	};

	return (
		<main class="min-h-screen bg-gray-50">
			<div class="max-w-5xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<div>
						<h1 class="text-2xl font-bold text-gray-900">Workspace Settings</h1>
						<p class="text-sm text-gray-500">Workspace ID: {workspaceId()}</p>
					</div>
					<div class="flex gap-2">
						<A
							href={`/workspaces/${workspaceId()}/notes`}
							class="px-3 py-1 text-sm bg-blue-600 text-white rounded hover:bg-blue-700"
						>
							Open Notes
						</A>
						<A
							href="/workspaces"
							class="px-3 py-1 text-sm border border-gray-300 rounded hover:bg-gray-50"
						>
							Back to Workspaces
						</A>
					</div>
				</div>

				<Show when={workspace.loading}>
					<p class="text-sm text-gray-500">Loading workspace...</p>
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
		</main>
	);
}
