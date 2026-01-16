import { useNavigate, useParams, useLocation } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import {
	createEffect,
	createMemo,
	createResource,
	createSignal,
	onMount,
	onCleanup,
} from "solid-js";
import { CreateNoteDialog, CreateClassDialog } from "~/components/create-dialogs";
import type { ClassCreatePayload } from "~/lib/types";
import { ListPanel } from "~/components/ListPanel";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { classApi, noteApi } from "~/lib/client";
import { ensureClassFrontmatter, replaceFirstH1 } from "~/lib/markdown";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { createNoteStore } from "~/lib/note-store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import type { NoteRecord } from "~/lib/types";

export default function NotesRoute(props: RouteSectionProps) {
	const navigate = useNavigate();
	const location = useLocation();
	const params = useParams<{ noteId?: string }>();

	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const noteStore = createNoteStore(workspaceId);

	// View mode: notes or classes derived from URL
	const viewMode = () => (location.pathname.includes("/notes/classes") ? "classes" : "notes");

	// Classes selection derived from URL
	const selectedClassName = () => {
		const parts = location.pathname.split("/");
		const classesIndex = parts.indexOf("classes");
		if (classesIndex !== -1 && parts.length > classesIndex + 1) {
			return decodeURIComponent(parts[classesIndex + 1]);
		}
		return null;
	};

	const selectedClass = () => {
		const name = selectedClassName();
		if (!name) return null;
		return classes()?.find((s) => s.name === name) || null;
	};

	// Filter state (notes only)
	const [filterClass, setFilterClass] = createSignal<string>("");

	// Search state
	const [searchQuery, setSearchQuery] = createSignal("");
	const [searchResults, setSearchResults] = createSignal<NoteRecord[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);

	// Dialogs
	const [showCreateNoteDialog, setShowCreateNoteDialog] = createSignal(false);
	const [showCreateClassDialog, setShowCreateClassDialog] = createSignal(false);

	const selectedNoteId = () => params.noteId ?? null;

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
	onMount(() => {
		workspaceStore.loadWorkspaces().catch(() => {
			// ignore
		});
	});

	onCleanup(() => {
		if (searchTimeoutId) clearTimeout(searchTimeoutId);
		if (searchAbortController) searchAbortController.abort();
	});

	// Load notes when workspace changes
	createEffect((prevWsId) => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			noteStore.loadNotes();
			setSearchResults([]);
			setIsSearching(false);

			// Only redirect to /notes if the workspace actually changed
			// and we're not already on a notes-related path
			if (prevWsId && prevWsId !== wsId) {
				navigate("/notes", { replace: true });
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
				const results = await noteApi.search(workspaceId(), query);
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
		workspaceStore.selectWorkspace(wsId);
	};

	const handleWorkspaceCreate = async (name: string) => {
		try {
			const ws = await workspaceStore.createWorkspace(name);
			workspaceStore.selectWorkspace(ws.id);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create workspace");
		}
	};

	const handleSelectNote = (noteId: string) => {
		navigate(`/notes/${noteId}`);
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
			navigate(`/notes/${result.id}`);
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
							onClick={() => navigate("/notes")}
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
							onClick={() => navigate("/notes/classes")}
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
						onSelectClass={(s) => navigate(`/notes/classes/${encodeURIComponent(s.name)}`)}
						selectedClass={selectedClass()}
					/>
				</aside>

				<div class="flex-1 flex flex-col overflow-hidden">{props.children}</div>

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
