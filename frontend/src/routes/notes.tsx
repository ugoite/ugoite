import { useNavigate, useParams } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import { createEffect, createMemo, createResource, createSignal, onMount, Show } from "solid-js";
import { CreateNoteDialog, CreateSchemaDialog } from "~/components/create-dialogs";
import { ListPanel } from "~/components/ListPanel";
import { SchemaTable } from "~/components/SchemaTable";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { schemaApi, noteApi } from "~/lib/client";
import { ensureClassFrontmatter, replaceFirstH1 } from "~/lib/markdown";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { createNoteStore } from "~/lib/store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import type { NoteRecord, Schema } from "~/lib/types";

export default function NotesRoute(props: RouteSectionProps) {
	const navigate = useNavigate();
	const params = useParams<{ noteId?: string }>();

	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const noteStore = createNoteStore(workspaceId);

	// View mode: notes or schemas (data models)
	const [viewMode, setViewMode] = createSignal<"notes" | "schemas">("notes");

	// Schemas selection
	const [selectedSchema, setSelectedSchema] = createSignal<Schema | null>(null);

	// Filter state (notes only)
	const [filterClass, setFilterClass] = createSignal<string>("");

	// Search state
	const [searchQuery, setSearchQuery] = createSignal("");
	const [searchResults, setSearchResults] = createSignal<NoteRecord[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);

	// Dialogs
	const [showCreateNoteDialog, setShowCreateNoteDialog] = createSignal(false);
	const [showCreateSchemaDialog, setShowCreateSchemaDialog] = createSignal(false);

	const selectedNoteId = () => params.noteId ?? null;

	// Schemas resource
	const [schemas, { refetch: refetchSchemas }] = createResource(
		() => {
			const wsId = workspaceId();
			return wsId ? wsId : null;
		},
		async (wsId) => {
			if (!wsId) return [];
			return await schemaApi.list(wsId);
		},
	);

	onMount(() => {
		workspaceStore.loadWorkspaces().catch(() => {
			// ignore
		});
	});

	// Load notes when workspace changes
	createEffect(() => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			noteStore.loadNotes();
			setSearchResults([]);
			setIsSearching(false);
			setSelectedSchema(null);
			setViewMode("notes");
			navigate("/notes", { replace: true });
		}
	});

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
		setViewMode("notes");
		navigate(`/notes/${noteId}`);
	};

	// Create note
	const handleCreateNote = async (title: string, className: string) => {
		const schema = (schemas() || []).find((s) => s.name === className);
		let initialContent = `# ${title}\n\nStart writing here...`;
		if (className && schema) {
			initialContent = ensureClassFrontmatter(replaceFirstH1(schema.template, title), className);
		}

		try {
			const result = await noteStore.createNote(initialContent);
			setShowCreateNoteDialog(false);
			setViewMode("notes");
			navigate(`/notes/${result.id}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create note");
		}
	};

	// Create schema
	const handleCreateSchema = async (name: string) => {
		try {
			await schemaApi.create(workspaceId(), {
				name,
				fields: {},
				template: `# New ${name}\n\n`,
			});
			setShowCreateSchemaDialog(false);
			refetchSchemas();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create data model");
		}
	};

	return (
		<NotesRouteContext.Provider
			value={{
				workspaceStore,
				workspaceId,
				noteStore,
				schemas: () => schemas() || [],
				refetchSchemas,
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
							onClick={() => setViewMode("notes")}
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
							onClick={() => {
								setViewMode("schemas");
								setSelectedSchema(null);
								navigate("/notes", { replace: true });
							}}
							class={`flex-1 py-3 text-sm font-medium text-center ${
								viewMode() === "schemas"
									? "text-blue-600 border-b-2 border-blue-600 bg-blue-50"
									: "text-gray-500 hover:text-gray-700 hover:bg-gray-50"
							}`}
						>
							Data Models
						</button>
					</div>

					<ListPanel
						mode={viewMode()}
						schemas={schemas() || []}
						filterClass={filterClass}
						onFilterClassChange={setFilterClass}
						onCreate={() =>
							viewMode() === "notes"
								? setShowCreateNoteDialog(true)
								: setShowCreateSchemaDialog(true)
						}
						onSearch={viewMode() === "notes" ? handleSearch : undefined}
						isSearching={isSearching()}
						searchResultsCount={searchResults().length}
						notes={viewMode() === "notes" ? displayNotes() : undefined}
						loading={noteStore.loading()}
						error={noteStore.error()}
						selectedId={selectedNoteId() || undefined}
						onSelectNote={handleSelectNote}
						onSelectSchema={setSelectedSchema}
						selectedSchema={selectedSchema()}
					/>
				</aside>

				<div class="flex-1 flex flex-col overflow-hidden">
					<Show when={viewMode() === "notes"}>{props.children}</Show>

					<Show when={viewMode() === "schemas"}>
						<Show
							when={selectedSchema()}
							fallback={
								<div class="flex-1 flex items-center justify-center text-gray-400">
									<div class="text-center">
										<svg
											class="w-16 h-16 mx-auto mb-4 opacity-50"
											fill="none"
											stroke="currentColor"
											viewBox="0 0 24 24"
											aria-hidden="true"
										>
											<path
												stroke-linecap="round"
												stroke-linejoin="round"
												stroke-width="1"
												d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 5c0 2.21-3.582 4-8 4s-8-1.79-8-4"
											/>
										</svg>
										<p>Select a data model to view records</p>
									</div>
								</div>
							}
						>
							{(schema) => (
								<SchemaTable
									workspaceId={workspaceId()}
									schema={schema()}
									onNoteClick={(noteId) => handleSelectNote(noteId)}
								/>
							)}
						</Show>
					</Show>
				</div>

				<CreateNoteDialog
					open={showCreateNoteDialog()}
					schemas={schemas() || []}
					onClose={() => setShowCreateNoteDialog(false)}
					onSubmit={handleCreateNote}
				/>

				<CreateSchemaDialog
					open={showCreateSchemaDialog()}
					onClose={() => setShowCreateSchemaDialog(false)}
					onSubmit={handleCreateSchema}
				/>
			</main>
		</NotesRouteContext.Provider>
	);
}
