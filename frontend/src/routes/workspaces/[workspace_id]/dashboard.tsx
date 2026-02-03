import { useParams } from "@solidjs/router";
import { createResource, Show } from "solid-js";
import { WorkspaceShell } from "~/components/WorkspaceShell";
import { workspaceApi } from "~/lib/workspace-api";

export default function WorkspaceDashboardRoute() {
	const params = useParams<{ workspace_id: string }>();
	const workspaceId = () => params.workspace_id;

	const [workspace] = createResource(async () => {
		return await workspaceApi.get(workspaceId());
	});

	return (
		<WorkspaceShell workspaceId={workspaceId()} activeTopTab="dashboard">
			<div class="mx-auto max-w-4xl">
				<Show when={workspace.loading}>
					<p class="text-sm text-slate-500">Loading workspace...</p>
				</Show>
				<Show when={workspace.error}>
					<p class="text-sm text-red-600">Failed to load workspace.</p>
				</Show>
				<Show when={workspace()}>
					{(ws) => <h1 class="text-4xl font-semibold text-slate-900">{ws().name}</h1>}
				</Show>
			</div>
		</WorkspaceShell>
	);
}
