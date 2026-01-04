import { useNavigate } from "@solidjs/router";
import { createResource, createSignal, Show, createEffect, onMount, For } from "solid-js";
import { NoteList } from "~/components/NoteList";
import { SchemaList } from "~/components/SchemaList";
import { SchemaTable } from "~/components/SchemaTable";
import { SearchBar } from "~/components/SearchBar";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { noteApi, schemaApi } from "~/lib/client";
import { createNoteStore } from "~/lib/store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import type { Schema, SearchResult } from "~/lib/types";

/**
 * Replace the first level-1 heading in a markdown template with the provided title.
 * If no H1 is present, prepend one to the template.
 *
 * @param template - The markdown template to modify
 * @param title - The replacement title to put in the first H1
 * @returns The modified markdown content
 */
import { replaceFirstH1, ensureClassFrontmatter } from "~/lib/markdown";

export default function NotesIndexPage() {
	const navigate = useNavigate();
	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const store = createNoteStore(workspaceId);

	const [viewMode, setViewMode] = createSignal<"notes" | "schemas">("notes");
	const [selectedSchema, setSelectedSchema] = createSignal<Schema | null>(null);
	const [schemas] = createResource(
		() => {
			const wsId = workspaceId();
			return wsId ? wsId : null;
		},
		async (wsId) => {
			if (!wsId) return [];
			return await schemaApi.list(wsId);
		},
	);

	const [newNoteClass, setNewNoteClass] = createSignal<string>("");

	// Search state
	const [searchResults, setSearchResults] = createSignal<SearchResult[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);

	onMount(() => {
		workspaceStore.loadWorkspaces().catch(() => {
			// ignore
		});
	});

	createEffect(() => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			store.loadNotes();
			setSearchResults([]);
			setIsSearching(false);
		}
	});

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

	// Execute search without blocking UI
	const executeSearch = async (query: string, controller: AbortController) => {
		try {
			const results = await noteApi.search(workspaceId(), query);
			if (!controller.signal.aborted) {
				setSearchResults(results);
			}
		} catch {
			if (!controller.signal.aborted) {
				setSearchResults([]);
			}
		} finally {
			if (!controller.signal.aborted) {
				setIsSearching(false);
			}
		}
	};

	let searchTimeoutId: ReturnType<typeof setTimeout> | null = null;
	let searchAbortController: AbortController | null = null;
	const handleSearch = (query: string) => {
		if (searchTimeoutId) {
			clearTimeout(searchTimeoutId);
		}
		if (searchAbortController) {
			searchAbortController.abort();
		}
		if (!query.trim()) {
			setSearchResults([]);
			setIsSearching(false);
			return;
		}
		setIsSearching(true);
		searchTimeoutId = setTimeout(() => {
			searchAbortController = new AbortController();
			const currentController = searchAbortController;
			Promise.resolve().then(() => executeSearch(query, currentController));
		}, 300);
	};

	const handleCreateNote = async () => {
		const title = prompt("Enter note title:");
		if (!title) return;

		const className = newNoteClass();
		const schema = (schemas() || []).find((s) => s.name === className);

		let initialContent = `# ${title}\n\nStart writing here...`;
		if (className && schema) {
			initialContent = ensureClassFrontmatter(replaceFirstH1(schema.template, title), className);
		}

		try {
			const result = await store.createNote(initialContent);
			navigate(`/notes/${result.id}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create note");
		}
	};

	return (
		<main class="flex h-screen overflow-hidden bg-gray-100">
			{/* Sidebar */}
			<aside class="w-80 flex-shrink-0 bg-white border-r flex flex-col">
				<WorkspaceSelector
					workspaces={workspaceStore.workspaces()}
					selectedWorkspaceId={workspaceStore.selectedWorkspaceId()}
					loading={workspaceStore.loading()}
					error={workspaceStore.error()}
					onSelect={handleWorkspaceSelect}
					onCreate={handleWorkspaceCreate}
				/>

				{/* View Mode Switcher */}
				<div class="flex border-b border-gray-200 dark:border-gray-800">
					<button
						type="button"
						onClick={() => setViewMode("notes")}
						class={`flex-1 py-3 text-sm font-medium text-center ${
							viewMode() === "notes"
								? "text-blue-600 border-b-2 border-blue-600 bg-blue-50 dark:bg-blue-900/20"
								: "text-gray-500 hover:text-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
						}`}
					>
						Notes
					</button>
					<button
						type="button"
						onClick={() => setViewMode("schemas")}
						class={`flex-1 py-3 text-sm font-medium text-center ${
							viewMode() === "schemas"
								? "text-blue-600 border-b-2 border-blue-600 bg-blue-50 dark:bg-blue-900/20"
								: "text-gray-500 hover:text-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
						}`}
					>
						Data Models
					</button>
				</div>

				<Show when={viewMode() === "notes"}>
					{/* Create Note */}
					<div class="p-4 border-b space-y-3">
						<button
							type="button"
							onClick={handleCreateNote}
							class="w-full px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 flex items-center justify-center gap-2"
						>
							<svg
								class="w-5 h-5"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
								aria-hidden="true"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M12 4v16m8-8H4"
								/>
							</svg>
							New Note
						</button>

						<Show when={(schemas() || []).length > 0}>
							<div>
								<label class="block text-xs font-medium text-gray-600 mb-1" for="new-note-class">
									Class (optional)
								</label>
								<select
									id="new-note-class"
									class="w-full px-3 py-2 border border-gray-300 rounded-lg bg-white text-gray-900"
									value={newNoteClass()}
									onChange={(e) => setNewNoteClass(e.currentTarget.value)}
								>
									<option value="">(none)</option>
									<For each={schemas() || []}>
										{(s) => <option value={s.name}>{s.name}</option>}
									</For>
								</select>
							</div>
						</Show>
					</div>

					{/* Search */}
					<div class="p-4 border-b">
						<SearchBar
							onSearch={handleSearch}
							loading={isSearching()}
							resultsCount={searchResults().length}
						/>
					</div>

					{/* Note List */}
					<div class="flex-1 overflow-auto p-4">
						<Show
							when={searchResults().length === 0}
							fallback={
								<NoteList
									notes={() => searchResults()}
									loading={() => isSearching()}
									error={() => null}
									onSelect={(noteId) => navigate(`/notes/${noteId}`)}
								/>
							}
						>
							<NoteList
								notes={store.notes}
								loading={store.loading}
								error={store.error}
								onSelect={(noteId) => navigate(`/notes/${noteId}`)}
							/>
						</Show>
					</div>
				</Show>

				<Show when={viewMode() === "schemas"}>
					<SchemaList
						schemas={schemas() || []}
						selectedSchema={selectedSchema()}
						onSelect={setSelectedSchema}
					/>
				</Show>
			</aside>

			{/* Main Content */}
			<div class="flex-1 flex flex-col overflow-hidden">
				<Show when={viewMode() === "notes"}>
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
									d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
								/>
							</svg>
							<p>Select a note to view</p>
						</div>
					</div>
				</Show>

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
								onNoteClick={(noteId) => navigate(`/notes/${noteId}`)}
							/>
						)}
					</Show>
				</Show>
			</div>
		</main>
	);
}
