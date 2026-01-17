import { A, useNavigate, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import {
	createEffect,
	createMemo,
	createResource,
	createSignal,
	onMount,
	onCleanup,
	Show,
} from "solid-js";
import { CreateNoteDialog, CreateClassDialog } from "~/components/create-dialogs";
import type { ClassCreatePayload } from "~/lib/types";
import { ListPanel } from "~/components/ListPanel";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { classApi } from "~/lib/class-api";
import { searchApi } from "~/lib/search-api";
import { ensureClassFrontmatter, replaceFirstH1 } from "~/lib/markdown";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { createNoteStore } from "~/lib/note-store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import type { NoteRecord } from "~/lib/types";

export default function WorkspaceNotesRoute(props: RouteSectionProps) {
	const navigate = useNavigate();
	const params = useParams<{ workspace_id: string; note_id?: string }>();

	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => params.workspace_id || "";

	const noteStore = createNoteStore(workspaceId);

	const viewMode = () => "notes" as const;

	// Filter state (notes only)
	const [filterClass, setFilterClass] = createSignal<string>("");

	// Search state
	const [searchQuery, setSearchQuery] = createSignal("");
	const [searchResults, setSearchResults] = createSignal<NoteRecord[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);

	// Dialogs
	const [showCreateNoteDialog, setShowCreateNoteDialog] = createSignal(false);
	const [showCreateClassDialog, setShowCreateClassDialog] = createSignal(false);

	const selectedNoteId = () => params.note_id ?? null;

	// Classes resource
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

	onMount(() => {
		workspaceStore.loadWorkspaces().catch(() => {
			// ignore
		});
	});

	onCleanup(() => {
		if (searchTimeoutId) clearTimeout(searchTimeoutId);
		if (searchAbortController) searchAbortController.abort();
	});

	// Sync workspace selection with route param
	createEffect(() => {
		const wsId = workspaceId();
		const list = workspaceStore.workspaces();
		if (!wsId || list.length === 0) return;
		if (workspaceStore.selectedWorkspaceId() !== wsId) {
			workspaceStore.selectWorkspace(wsId);
		}
	});

	// Load notes when workspace changes
	createEffect((prevWsId) => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			noteStore.loadNotes();
			setSearchResults([]);
			setIsSearching(false);

			if (prevWsId && prevWsId !== wsId) {
				navigate(`/workspaces/${wsId}/notes`, { replace: true });
			}
		}
		return wsId;
	}, "");

	// Computed: filtered notes for display
	const displayNotes = createMemo(() => {
		// If searching, use search results
		if (searchQuery().trim()) {
			return filterClass()
				? searchResults().filter((n) => n.class === filterClass())
				: searchResults();
		}
		// Otherwise use store notes, filtered by class if set
		const notes = noteStore.notes() || [];
		return filterClass() ? notes.filter((n) => n.class === filterClass()) : notes;
	});

	// Search handler
	let searchTimeoutId: ReturnType<typeof setTimeout> | null = null;
	let searchAbortController: AbortController | null = null;

	const handleSearch = (query: string) => {
		setSearchQuery(query);
		if (searchTimeoutId) clearTimeout(searchTimeoutId);
		if (searchAbortController) searchAbortController.abort();

		if (!query.trim()) {
			setSearchResults([]);
			setIsSearching(false);
			return;
		}

		setIsSearching(true);
		searchTimeoutId = setTimeout(async () => {
			searchAbortController = new AbortController();
			try {
				const results = await searchApi.keyword(workspaceId(), query);
				if (!searchAbortController.signal.aborted) {
					setSearchResults(results as NoteRecord[]);
				}
			} catch {
				if (!searchAbortController?.signal.aborted) {
					setSearchResults([]);
				}
			} finally {
				if (!searchAbortController?.signal.aborted) {
					setIsSearching(false);
				}
			}
		}, 300);
	};

	const handleWorkspaceSelect = (wsId: string) => {
		navigate(`/workspaces/${wsId}/notes`);
	};

	const handleWorkspaceCreate = async (name: string) => {
		try {
			const ws = await workspaceStore.createWorkspace(name);
			navigate(`/workspaces/${ws.id}/notes`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create workspace");
		}
	};

	const handleSelectNote = (noteId: string) => {
		navigate(`/workspaces/${workspaceId()}/notes/${noteId}`);
	};

	// Create note
	const handleCreateNote = async (title: string, className: string) => {
		const classDef = (classes() || []).find((s) => s.name === className);
		let initialContent = `# ${title}\n\nStart writing here...`;
		if (className && classDef) {
			initialContent = ensureClassFrontmatter(replaceFirstH1(classDef.template, title), className);
		}

		try {
			const result = await noteStore.createNote(initialContent);
			setShowCreateNoteDialog(false);
			navigate(`/workspaces/${workspaceId()}/notes/${result.id}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create note");
		}
	};

	// Create class
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
					</div>

					<ListPanel
						mode={viewMode()}
						classes={classes() || []}
						filterClass={filterClass}
						onFilterClassChange={setFilterClass}
						onCreate={() =>
							viewMode() === "notes"
								? setShowCreateNoteDialog(true)
								: setShowCreateClassDialog(true)
						}
						onSearch={viewMode() === "notes" ? handleSearch : undefined}
						isSearching={isSearching()}
						searchResultsCount={searchResults().length}
						notes={viewMode() === "notes" ? displayNotes() : undefined}
						loading={noteStore.loading()}
						error={noteStore.error()}
						selectedId={selectedNoteId() || undefined}
						onSelectNote={handleSelectNote}
						onSelectClass={(s) =>
							navigate(`/workspaces/${workspaceId()}/classes/${encodeURIComponent(s.name)}`)
						}
						selectedClass={null}
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

				<CreateNoteDialog
					open={showCreateNoteDialog()}
					classes={classes() || []}
					onClose={() => setShowCreateNoteDialog(false)}
					onSubmit={handleCreateNote}
				/>

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
