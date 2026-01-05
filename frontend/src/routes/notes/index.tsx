import { createResource, createSignal, Show, createEffect, onMount, createMemo } from "solid-js";
import { AttachmentUploader } from "~/components/AttachmentUploader";
import { CreateNoteDialog, CreateSchemaDialog } from "~/components/create-dialogs";
import { ListPanel } from "~/components/ListPanel";
import { MarkdownEditor } from "~/components/MarkdownEditor";
import { SchemaTable } from "~/components/SchemaTable";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { attachmentApi, noteApi, schemaApi, RevisionConflictError } from "~/lib/client";
import { replaceFirstH1, ensureClassFrontmatter } from "~/lib/markdown";
import { createNoteStore } from "~/lib/store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import type { Attachment, NoteRecord } from "~/lib/types";

export default function NotesIndexPage() {
	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const store = createNoteStore(workspaceId);

	// View mode: notes or schemas (data models)
	const [viewMode, setViewMode] = createSignal<"notes" | "schemas">("notes");

	// Selected items
	const [selectedNoteId, setSelectedNoteId] = createSignal<string | null>(null);
	const [selectedSchema, setSelectedSchema] = createSignal<{
		name: string;
		fields: Record<string, { type: string }>;
		template: string;
	} | null>(null);

	// Filter state
	const [filterClass, setFilterClass] = createSignal<string>("");

	// Search state
	const [searchQuery, setSearchQuery] = createSignal("");
	const [searchResults, setSearchResults] = createSignal<NoteRecord[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);

	// Dialogs
	const [showCreateNoteDialog, setShowCreateNoteDialog] = createSignal(false);
	const [showCreateSchemaDialog, setShowCreateSchemaDialog] = createSignal(false);

	// Editor state
	const [editorContent, setEditorContent] = createSignal("");
	const [isDirty, setIsDirty] = createSignal(false);
	const [isSaving, setIsSaving] = createSignal(false);
	const [conflictMessage, setConflictMessage] = createSignal<string | null>(null);
	const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(null);
	const [lastLoadedNoteId, setLastLoadedNoteId] = createSignal<string | null>(null);

	// Attachments
	const [attachments, setAttachments] = createSignal<Attachment[]>([]);

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

	// Selected note resource
	const [selectedNote, { refetch: refetchNote }] = createResource(
		() => {
			const wsId = workspaceId();
			const noteId = selectedNoteId();
			return wsId && noteId ? { wsId, noteId } : null;
		},
		async (p) => {
			if (!p) return null;
			try {
				return await noteApi.get(p.wsId, p.noteId);
			} catch {
				return null;
			}
		},
	);

	// Initialize workspace
	onMount(() => {
		workspaceStore.loadWorkspaces().catch(() => {
			// ignore
		});
	});

	// Load notes when workspace changes
	createEffect(() => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			store.loadNotes();
			setSearchResults([]);
			setIsSearching(false);
			setSelectedNoteId(null);
			setSelectedSchema(null);
			// Load attachments
			attachmentApi
				.list(wsId)
				.then(setAttachments)
				.catch(() => setAttachments([]));
		}
	});

	// Sync editor content when note changes
	createEffect(() => {
		const note = selectedNote();
		if (note && note.id !== lastLoadedNoteId()) {
			setLastLoadedNoteId(note.id);
			setCurrentRevisionId(note.revision_id);
			setEditorContent(note.content ?? "");
			setIsDirty(false);
			setConflictMessage(null);
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
		const notes = store.notes() || [];
		return filterClass() ? notes.filter((n) => n.class === filterClass()) : notes;
	});

	// Handlers
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

	// Note selection
	const handleSelectNote = (noteId: string) => {
		setSelectedNoteId(noteId);
		setLastLoadedNoteId(null); // Force reload
	};

	// Create note
	const handleCreateNote = async (title: string, className: string) => {
		const schema = (schemas() || []).find((s) => s.name === className);
		let initialContent = `# ${title}\n\nStart writing here...`;
		if (className && schema) {
			initialContent = ensureClassFrontmatter(replaceFirstH1(schema.template, title), className);
		}

		try {
			const result = await store.createNote(initialContent);
			setShowCreateNoteDialog(false);
			setSelectedNoteId(result.id);
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

	// Editor handlers
	const handleContentChange = (content: string) => {
		setEditorContent(content);
		setIsDirty(true);
		setConflictMessage(null);
	};

	const handleSave = async () => {
		const wsId = workspaceId();
		const noteId = selectedNoteId();
		const revisionId = currentRevisionId() || selectedNote()?.revision_id;

		if (!wsId || !noteId || !revisionId) {
			setConflictMessage("Cannot save: note not properly loaded. Please try refreshing.");
			return;
		}

		setIsSaving(true);
		setConflictMessage(null);

		try {
			const result = await noteApi.update(wsId, noteId, {
				markdown: editorContent(),
				parent_revision_id: revisionId,
			});
			setCurrentRevisionId(result.revision_id);
			setIsDirty(false);
			// Reload note list to reflect changes
			store.loadNotes();
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

	const handleDelete = async () => {
		const wsId = workspaceId();
		const noteId = selectedNoteId();
		if (!wsId || !noteId) return;
		if (!confirm("Are you sure you want to delete this note?")) return;

		try {
			await noteApi.delete(wsId, noteId);
			setSelectedNoteId(null);
			store.loadNotes();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to delete note");
		}
	};

	const handleAttachmentUpload = async (file: File): Promise<Attachment> => {
		const attachment = await attachmentApi.upload(workspaceId(), file);
		const wsId = workspaceId();
		if (wsId) {
			try {
				setAttachments(await attachmentApi.list(wsId));
			} catch {
				// ignore
			}
		}
		return attachment;
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
						onClick={() => setViewMode("schemas")}
						class={`flex-1 py-3 text-sm font-medium text-center ${
							viewMode() === "schemas"
								? "text-blue-600 border-b-2 border-blue-600 bg-blue-50"
								: "text-gray-500 hover:text-gray-700 hover:bg-gray-50"
						}`}
					>
						Data Models
					</button>
				</div>

				{/* List Panel */}
				<ListPanel
					mode={viewMode()}
					schemas={schemas() || []}
					filterClass={filterClass}
					onFilterClassChange={setFilterClass}
					onCreate={() =>
						viewMode() === "notes" ? setShowCreateNoteDialog(true) : setShowCreateSchemaDialog(true)
					}
					onSearch={viewMode() === "notes" ? handleSearch : undefined}
					isSearching={isSearching()}
					searchResultsCount={searchResults().length}
					notes={viewMode() === "notes" ? displayNotes() : undefined}
					loading={store.loading()}
					error={store.error()}
					selectedId={selectedNoteId() || undefined}
					onSelectNote={handleSelectNote}
					onSelectSchema={setSelectedSchema}
					selectedSchema={selectedSchema()}
				/>
			</aside>

			{/* Main Content */}
			<div class="flex-1 flex flex-col overflow-hidden">
				<Show when={viewMode() === "notes"}>
					<Show
						when={selectedNoteId() && selectedNote()}
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
									<p>Select a note to view</p>
								</div>
							</div>
						}
					>
						{/* Note Editor */}
						<div class="bg-white border-b px-4 py-3 flex items-center justify-between">
							<div>
								<h2 class="font-semibold text-gray-800">{selectedNote()?.title || "Untitled"}</h2>
								<Show when={selectedNote()?.class}>
									<span class="text-sm text-gray-500">Class: {selectedNote()?.class}</span>
								</Show>
							</div>
							<div class="flex items-center gap-2">
								<button
									type="button"
									onClick={() => refetchNote()}
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
								<button
									type="button"
									onClick={handleDelete}
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
						</div>

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

							<div class="border-t p-4">
								<AttachmentUploader onUpload={handleAttachmentUpload} attachments={attachments()} />
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
									setSelectedNoteId(noteId);
								}}
							/>
						)}
					</Show>
				</Show>
			</div>

			{/* Dialogs */}
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
	);
}
