import { createSignal, Show, createEffect, onMount, untrack, createResource } from "solid-js";
import { createNoteStore } from "~/lib/store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import { NoteList } from "~/components/NoteList";
import { MarkdownEditor } from "~/components/MarkdownEditor";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { SearchBar } from "~/components/SearchBar";
import { AttachmentUploader } from "~/components/AttachmentUploader";
import { RevisionConflictError, noteApi, attachmentApi, schemaApi } from "~/lib/client";
import type { SearchResult, Attachment, Schema } from "~/lib/types";
import { SchemaList } from "~/components/SchemaList";
import { SchemaTable } from "~/components/SchemaTable";

export default function NotesPage() {
	// Workspace store manages workspace selection
	const workspaceStore = createWorkspaceStore();

	// Reactive workspace ID that drives note loading
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const [showEditor, setShowEditor] = createSignal(false);
	const [viewMode, setViewMode] = createSignal<"notes" | "schemas">("notes");
	const [selectedSchema, setSelectedSchema] = createSignal<Schema | null>(null);
	const [schemas] = createResource(workspaceId, schemaApi.list);

	const store = createNoteStore(workspaceId);

	// Search state
	const [searchResults, setSearchResults] = createSignal<SearchResult[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);

	// Scroll position state - store it reactively
	const [scrollPosition, setScrollPosition] = createSignal(0);
	let noteListContainerRef: HTMLDivElement | undefined;

	// Attachments state
	const [attachments, setAttachments] = createSignal<Attachment[]>([]);

	// Local editor state
	const [editorContent, setEditorContent] = createSignal("");
	const [isDirty, setIsDirty] = createSignal(false);
	const [isSaving, setIsSaving] = createSignal(false);
	const [conflictMessage, setConflictMessage] = createSignal<string | null>(null);
	// Track the current revision ID locally to support consecutive saves
	// This is updated after each successful save to avoid using stale revision_id
	const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(null);

	// Load notes on mount
	onMount(() => {
		// Initialize workspace store first, which loads/creates default workspace
		workspaceStore.loadWorkspaces().catch(() => {
			// Silently ignore workspace load errors - the UI will show appropriate state
		});
	});

	// Load notes when workspace changes
	createEffect(() => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			// Reset editor state when switching workspaces
			setShowEditor(false);
			setEditorContent("");
			setIsDirty(false);
			setCurrentRevisionId(null);
			setLastLoadedNoteId(null); // Reset to allow loading first note in new workspace
			store.selectNote(null);
			store.loadNotes();

			// Load attachments for the workspace
			loadAttachments(wsId);
		}
	});

	// Load attachments from the workspace
	const loadAttachments = async (wsId: string) => {
		try {
			const attachmentList = await attachmentApi.list(wsId);
			setAttachments(attachmentList);
		} catch (_e) {
			// Silently ignore attachment load errors
			setAttachments([]);
		}
	};

	// Sync editor content when selected note changes (only on initial load or note switch)
	// Track the last loaded note ID to avoid re-syncing when resource refetches
	const [lastLoadedNoteId, setLastLoadedNoteId] = createSignal<string | null>(null);
	createEffect(() => {
		const note = store.selectedNote();
		if (note && note.id !== lastLoadedNoteId()) {
			// Only update editor content when switching to a different note
			// This prevents overwriting user's edits after save or refetch
			setLastLoadedNoteId(note.id);
			setCurrentRevisionId(note.revision_id);
			setEditorContent(note.content ?? "");
			setIsDirty(false);
			setConflictMessage(null);
			setShowEditor(true);
		}
	});

	const handleNoteSelect = (noteId: string) => {
		if (isDirty()) {
			// Simple confirmation - in production, use a proper modal
			if (!confirm("You have unsaved changes. Discard them?")) {
				return;
			}
		}

		// Save current scroll position before state change
		setScrollPosition(noteListContainerRef?.scrollTop ?? 0);

		store.selectNote(noteId);
	};

	// Restore scroll position after render (but untrack to avoid infinite loops)
	createEffect(() => {
		// Track selected note to know when selection changes
		store.selectedNoteId();

		// Untrack the scroll restoration to prevent infinite loops
		untrack(() => {
			const savedScroll = scrollPosition();
			if (noteListContainerRef && savedScroll > 0) {
				// Use multiple strategies to ensure scroll is restored
				noteListContainerRef.scrollTop = savedScroll;

				// Also try after next frame in case layout is still pending
				requestAnimationFrame(() => {
					if (noteListContainerRef) {
						noteListContainerRef.scrollTop = savedScroll;
					}
				});
			}
		});
	});

	const handleContentChange = (content: string) => {
		setEditorContent(content);
		setIsDirty(true);
		setConflictMessage(null);
	};

	const handleSave = async () => {
		const noteId = store.selectedNoteId();
		// Use local revision ID if available, fall back to server's version
		const revisionId = currentRevisionId() || store.selectedNote()?.revision_id;

		if (!noteId || !revisionId) {
			setConflictMessage("Cannot save: note not properly loaded. Please try refreshing.");
			return;
		}

		setIsSaving(true);
		setConflictMessage(null);

		try {
			const result = await store.updateNote(noteId, {
				markdown: editorContent(),
				parent_revision_id: revisionId,
			});
			// Update local revision ID to support consecutive saves
			setCurrentRevisionId(result.revision_id);
			setIsDirty(false);
		} catch (e) {
			if (e instanceof RevisionConflictError) {
				setConflictMessage(
					"This note was modified elsewhere. Please refresh to see the latest version.",
				);
			} else {
				setConflictMessage(e instanceof Error ? e.message : "Failed to save");
			}
		} finally {
			setIsSaving(false);
		}
	};

	const handleCreateNote = async () => {
		const title = prompt("Enter note title:");
		if (!title) return;

		const initialContent = `# ${title}\n\nStart writing here...`;
		try {
			const result = await store.createNote(initialContent);
			// Set editor state immediately with the new revision_id
			// This is critical for enabling saves immediately after creation
			setCurrentRevisionId(result.revision_id);
			setLastLoadedNoteId(result.id);
			setEditorContent(initialContent);
			setShowEditor(true);
			setIsDirty(false);
			setConflictMessage(null);
			store.selectNote(result.id);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create note");
		}
	};

	const handleDeleteNote = async () => {
		const noteId = store.selectedNoteId();
		if (!noteId) return;

		if (!confirm("Are you sure you want to delete this note?")) {
			return;
		}

		try {
			await store.deleteNote(noteId);
			setShowEditor(false);
			setEditorContent("");
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to delete note");
		}
	};

	const handleRefresh = () => {
		store.loadNotes();
		if (store.selectedNoteId()) {
			store.refetchSelectedNote();
		}
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

	// Execute search without blocking UI
	const executeSearch = async (query: string, controller: AbortController) => {
		try {
			const results = await noteApi.search(workspaceId(), query);
			// Only update if this search wasn't aborted
			if (!controller.signal.aborted) {
				setSearchResults(results);
			}
		} catch (_e) {
			// Ignore aborted searches or other errors
			if (!controller.signal.aborted) {
				setSearchResults([]);
			}
		} finally {
			if (!controller.signal.aborted) {
				setIsSearching(false);
			}
		}
	};

	// Search handler with async debouncing and AbortController to prevent blocking
	let searchTimeoutId: ReturnType<typeof setTimeout> | null = null;
	let searchAbortController: AbortController | null = null;
	const handleSearch = (query: string) => {
		// Clear previous timeout
		if (searchTimeoutId) {
			clearTimeout(searchTimeoutId);
		}

		// Abort any ongoing search request
		if (searchAbortController) {
			searchAbortController.abort();
		}

		// If query is empty, clear results immediately
		if (!query.trim()) {
			setSearchResults([]);
			setIsSearching(false);
			return;
		}

		// Set loading state immediately for UI feedback
		setIsSearching(true);

		// Debounce the actual search call
		searchTimeoutId = setTimeout(() => {
			// Create new abort controller for this search
			searchAbortController = new AbortController();
			const currentController = searchAbortController;

			// Execute search in a completely non-blocking way
			Promise.resolve().then(() => executeSearch(query, currentController));
		}, 300); // 300ms debounce delay
	};

	// Attachment upload handler
	const handleAttachmentUpload = async (file: File): Promise<Attachment> => {
		const attachment = await attachmentApi.upload(workspaceId(), file);
		// Reload attachments after upload to ensure list is up to date
		await loadAttachments(workspaceId());
		return attachment;
	};

	return (
		<main class="flex h-screen overflow-hidden bg-gray-100">
			{/* Sidebar */}
			<aside class="w-80 flex-shrink-0 bg-white border-r flex flex-col">
				{/* Workspace Selector */}
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
					{/* Header */}
					<div class="p-4 border-b">
						<div class="flex items-center justify-between mb-4">
							<h1 class="text-xl font-bold text-gray-800">IEapp</h1>
							<button
								type="button"
								onClick={handleRefresh}
								class="p-2 hover:bg-gray-100 rounded"
								aria-label="Refresh"
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
										d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
									/>
								</svg>
							</button>
						</div>
					</div>

					{/* Create Note Button */}
					<div class="p-4 border-b">
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
					</div>

					{/* Search Bar */}
					<div class="p-4 border-b">
						<SearchBar
							onSearch={handleSearch}
							loading={isSearching()}
							resultsCount={searchResults().length}
						/>
					</div>

					{/* Note List (or Search Results) */}
					<div
						ref={noteListContainerRef}
						class="flex-1 overflow-auto p-4"
						onScroll={(e) => setScrollPosition(e.currentTarget.scrollTop)}
					>
						<Show
							when={searchResults().length === 0}
							fallback={
								<NoteList
									notes={() => searchResults()}
									loading={isSearching}
									error={() => null}
									selectedNoteId={store.selectedNoteId() || undefined}
									onSelect={handleNoteSelect}
								/>
							}
						>
							<NoteList
								notes={store.notes}
								loading={store.loading}
								error={store.error}
								selectedNoteId={store.selectedNoteId() || undefined}
								onSelect={handleNoteSelect}
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
					<Show
						when={showEditor() && store.selectedNote()}
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
											d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
										/>
									</svg>
									<p>Select a note to edit</p>
									<p class="text-sm mt-1">or create a new one</p>
								</div>
							</div>
						}
					>
						<div class="flex-1 flex flex-col overflow-hidden">
							{/* Note Header */}
							<div class="bg-white border-b px-4 py-3 flex items-center justify-between">
								<div>
									<h2 class="font-semibold text-gray-800">
										{store.notes().find((n) => n.id === store.selectedNoteId())?.title ||
											"Untitled"}
									</h2>
									<Show when={store.notes().find((n) => n.id === store.selectedNoteId())?.class}>
										<span class="text-sm text-gray-500">
											Class: {store.notes().find((n) => n.id === store.selectedNoteId())?.class}
										</span>
									</Show>
								</div>
								<button
									type="button"
									onClick={handleDeleteNote}
									class="p-2 text-red-500 hover:bg-red-50 rounded"
									aria-label="Delete note"
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
											d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
										/>
									</svg>
								</button>
							</div>

							{/* Editor */}
							<div class="flex-1 bg-white overflow-hidden flex flex-col">
								<MarkdownEditor
									content={editorContent()}
									onChange={handleContentChange}
									onSave={handleSave}
									isDirty={isDirty()}
									isSaving={isSaving()}
									conflictMessage={conflictMessage() || undefined}
									showPreview
									placeholder="Start writing in Markdown..."
								/>

								{/* Attachments Section */}
								<div class="border-t p-4">
									<AttachmentUploader
										onUpload={handleAttachmentUpload}
										attachments={attachments()}
									/>
								</div>
							</div>
						</div>
					</Show>
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
								onNoteClick={(noteId) => {
									setViewMode("notes");
									store.selectNote(noteId);
									setShowEditor(true);
								}}
							/>
						)}
					</Show>
				</Show>
			</div>
		</main>
	);
}
