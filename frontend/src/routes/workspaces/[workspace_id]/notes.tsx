import { useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createMemo, createResource } from "solid-js";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { classApi } from "~/lib/class-api";
import { createNoteStore } from "~/lib/note-store";
import { createWorkspaceStore } from "~/lib/workspace-store";

export default function WorkspaceNotesRoute(props: RouteSectionProps) {
	const params = useParams<{ workspace_id: string }>();
	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => params.workspace_id || "";
	const noteStore = createNoteStore(workspaceId);

	const [classes, { refetch: refetchClasses }] = createResource(
		() => {
			const wsId = workspaceId();
			return wsId ? wsId : null;
		},
		async (wsId) => {
			if (!wsId) return [];
			return await classApi.list(wsId);
		},
	);

	const [columnTypes] = createResource(
		() => workspaceId(),
		async (wsId) => {
			if (!wsId) return [];
			return await classApi.listTypes(wsId);
		},
	);

	const safeClasses = createMemo(() => classes() || []);
	const loadingClasses = createMemo(() => classes.loading);

	return (
		<NotesRouteContext.Provider
			value={{
				workspaceStore,
				workspaceId,
				noteStore,
				classes: safeClasses,
				loadingClasses,
				columnTypes: () => columnTypes() || [],
				refetchClasses,
			}}
		>
			{props.children}
		</NotesRouteContext.Provider>
	);
}
