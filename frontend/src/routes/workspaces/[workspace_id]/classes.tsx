import { A, useNavigate, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createEffect, createMemo, createResource, createSignal, onMount, Show } from "solid-js";
import { CreateClassDialog } from "~/components/create-dialogs";
import type { ClassCreatePayload } from "~/lib/types";
import { ListPanel } from "~/components/ListPanel";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { classApi } from "~/lib/class-api";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { createNoteStore } from "~/lib/note-store";
import { createWorkspaceStore } from "~/lib/workspace-store";

export default function WorkspaceClassesRoute(props: RouteSectionProps) {
	const navigate = useNavigate();
	const params = useParams<{ workspace_id: string; class_name?: string }>();

	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => params.workspace_id || "";

	const noteStore = createNoteStore(workspaceId);

	const viewMode = () => "classes" as const;

	const [filterClass, setFilterClass] = createSignal<string>("");
	const [showCreateClassDialog, setShowCreateClassDialog] = createSignal(false);

	const selectedClassName = () => params.class_name ?? null;

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

	const workspaceExists = createMemo(() => {
		const wsId = workspaceId();
		if (!wsId) return false;
		return workspaceStore.workspaces().some((w) => w.id === wsId);
	});

	const selectedClass = createMemo(() => {
		const name = selectedClassName();
		if (!name) return null;
		return classes()?.find((s) => s.name === name) || null;
	});

	onMount(() => {
		workspaceStore.loadWorkspaces().catch(() => {
			// ignore
		});
	});

	createEffect(() => {
		const wsId = workspaceId();
		const list = workspaceStore.workspaces();
		if (!wsId || list.length === 0) return;
		if (workspaceStore.selectedWorkspaceId() !== wsId) {
			workspaceStore.selectWorkspace(wsId);
		}
	});

	createEffect((prevWsId) => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			if (prevWsId && prevWsId !== wsId) {
				navigate(`/workspaces/${wsId}/classes`, { replace: true });
			}
		}
		return wsId;
	}, "");

	const handleWorkspaceSelect = (wsId: string) => {
		navigate(`/workspaces/${wsId}/classes`);
	};

	const handleWorkspaceCreate = async (name: string) => {
		try {
			const ws = await workspaceStore.createWorkspace(name);
			navigate(`/workspaces/${ws.id}/classes`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create workspace");
		}
	};

	const handleCreateClass = async (payload: ClassCreatePayload) => {
		try {
			await classApi.create(workspaceId(), payload);
			setShowCreateClassDialog(false);
			refetchClasses();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create class");
		}
	};

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
			<main class="flex h-screen overflow-hidden bg-gray-100">
				<aside class="w-80 flex-shrink-0 bg-white border-r flex flex-col">
					<WorkspaceSelector
						workspaces={workspaceStore.workspaces()}
						selectedWorkspaceId={workspaceStore.selectedWorkspaceId()}
						loading={workspaceStore.loading()}
						error={workspaceStore.error()}
						onSelect={handleWorkspaceSelect}
						onCreate={handleWorkspaceCreate}
					/>

					<div class="flex border-b border-gray-200">
						<button
							type="button"
							onClick={() => navigate(`/workspaces/${workspaceId()}/notes`)}
							class={`flex-1 py-3 text-sm font-medium text-center ${
								viewMode() === "notes"
									? "text-blue-600 border-b-2 border-blue-600 bg-blue-50"
									: "text-gray-500 hover:text-gray-700 hover:bg-gray-50"
							}`}
						>
							Notes
						</button>
						<button
							type="button"
							onClick={() => navigate(`/workspaces/${workspaceId()}/classes`)}
							class={`flex-1 py-3 text-sm font-medium text-center ${
								viewMode() === "classes"
									? "text-blue-600 border-b-2 border-blue-600 bg-blue-50"
									: "text-gray-500 hover:text-gray-700 hover:bg-gray-50"
							}`}
						>
							Classes
						</button>
						<button
							type="button"
							onClick={() => navigate(`/workspaces/${workspaceId()}/query`)}
							class="flex-1 py-3 text-sm font-medium text-center text-gray-500 hover:text-gray-700 hover:bg-gray-50"
						>
							Query
						</button>
					</div>

					<ListPanel
						mode={viewMode()}
						classes={classes() || []}
						filterClass={filterClass}
						onFilterClassChange={setFilterClass}
						onCreate={() => setShowCreateClassDialog(true)}
						loading={noteStore.loading()}
						error={noteStore.error()}
						onSelectClass={(s) =>
							navigate(`/workspaces/${workspaceId()}/classes/${encodeURIComponent(s.name)}`)
						}
						selectedClass={selectedClass()}
					/>
				</aside>

				<div class="flex-1 flex flex-col overflow-hidden">
					<Show when={!workspaceExists() && workspaceStore.initialized()}>
						<div class="p-6 bg-white border-b">
							<p class="text-sm text-red-600">Workspace {workspaceId()} not found.</p>
							<A href="/workspaces" class="text-sm text-sky-700 hover:underline">
								Back to workspaces
							</A>
						</div>
					</Show>
					{props.children}
				</div>

				<CreateClassDialog
					open={showCreateClassDialog()}
					columnTypes={columnTypes() || []}
					onClose={() => setShowCreateClassDialog(false)}
					onSubmit={handleCreateClass}
				/>
			</main>
		</NotesRouteContext.Provider>
	);
}
