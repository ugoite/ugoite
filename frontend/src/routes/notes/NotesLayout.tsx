import { useNavigate } from "@solidjs/router";
import {
	createResource,
	createSignal,
	Show,
	createEffect,
	onMount,
	For,
	createMemo,
} from "solid-js";
import { NoteList } from "~/components/NoteList";
import { SchemaTable } from "~/components/SchemaTable";
import { SearchBar } from "~/components/SearchBar";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { CreateNoteDialog, CreateSchemaDialog } from "~/components/create-dialogs";
import { AttachmentUploader } from "~/components/AttachmentUploader";
import { MarkdownEditor } from "~/components/MarkdownEditor";
import { noteApi, schemaApi, attachmentApi, RevisionConflictError } from "~/lib/client";
import { createNoteStore } from "~/lib/store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import type { Schema, SearchResult, Attachment } from "~/lib/types";
import { replaceFirstH1, ensureClassFrontmatter } from "~/lib/markdown";

interface NotesLayoutProps {
	/** Optional note ID to display */
	noteId?: string;
}

/**
 * Main Notes layout component with split-pane view.
 * Left: Sidebar with workspace, search, filters, and note/schema list.
 * Right: Note editor or schema table view.
 */
export default function NotesLayout(props: NotesLayoutProps) {
	const navigate = useNavigate();
	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const store = createNoteStore(workspaceId);

	const [viewMode, setViewMode] = createSignal<"notes" | "schemas">("notes");
	const [selectedSchemaName, setSelectedSchemaName] = createSignal<string | null>(null);

	// Dialog states
	const [showCreateNoteDialog, setShowCreateNoteDialog] = createSignal(false);
	const [showCreateSchemaDialog, setShowCreateSchemaDialog] = createSignal(false);

	// Class filter for notes view
	const [classFilter, setClassFilter] = createSignal<string>("");

	// Search state
	const [searchQuery, setSearchQuery] = createSignal("");
	const [searchResults, setSearchResults] = createSignal<SearchResult[]>([]);
	const [isSearching, setIsSearching] = createSignal(false);

	// Note editor state
	const [editorContent, setEditorContent] = createSignal("");
	const [isDirty, setIsDirty] = createSignal(false);
	const [isSaving, setIsSaving] = createSignal(false);
	const [conflictMessage, setConflictMessage] = createSignal<string | null>(null);
	const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(null);
	const [lastLoadedNoteId, setLastLoadedNoteId] = createSignal<string | null>(null);
	const [attachments, setAttachments] = createSignal<Attachment[]>([]);

	// Load schemas
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

	// Load current note
	const [note, { refetch: refetchNote }] = createResource(
		() => {
			const wsId = workspaceId();
			const noteId = props.noteId;
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

	// Get selected schema object
	const selectedSchema = createMemo(() => {
		const name = selectedSchemaName();
		if (!name) return null;
		return (schemas() || []).find((s) => s.name === name) || null;
	});

	// Filter notes by class
	const filteredNotes = createMemo(() => {
		const filter = classFilter();
		const notes = searchResults().length > 0 ? searchResults() : store.notes();
		if (!filter) return notes;
		return notes.filter((n) => n.class === filter);
	});

	// Filter schemas by search
	const filteredSchemas = createMemo(() => {
		const query = searchQuery().toLowerCase().trim();
		const allSchemas = schemas() || [];
		if (!query) return allSchemas;
		return allSchemas.filter(
			(s) =>
				s.name.toLowerCase().includes(query) ||
				Object.keys(s.fields).some((f) => f.toLowerCase().includes(query)),
		);
	});

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
			setSearchQuery("");
		}
	});

	// Load attachments when workspace changes
	createEffect(() => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			attachmentApi
				.list(wsId)
				.then(setAttachments)
				.catch(() => setAttachments([]));
		}
	});

	// Load note content when note changes
	createEffect(() => {
		const n = note();
		if (n && n.id !== lastLoadedNoteId()) {
			setLastLoadedNoteId(n.id);
			setCurrentRevisionId(n.revision_id);
			setEditorContent(n.content ?? "");
			setIsDirty(false);
			setConflictMessage(null);
		}
	});

	const handleWorkspaceSelect = (wsId: string) => {
		workspaceStore.selectWorkspace(wsId);
		// Clear note selection when workspace changes
		if (props.noteId) {
			navigate("/notes");
		}
	};

	const handleWorkspaceCreate = async (name: string) => {
		try {
			const ws = await workspaceStore.createWorkspace(name);
			workspaceStore.selectWorkspace(ws.id);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create workspace");
		}
	};

	// Search execution
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
		setSearchQuery(query);

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

	// Note creation
	const handleCreateNote = async (title: string, className: string) => {
		const schema = (schemas() || []).find((s) => s.name === className);

		let initialContent = `# ${title}\n\nStart writing here...`;
		if (className && schema) {
			initialContent = ensureClassFrontmatter(replaceFirstH1(schema.template, title), className);
		}

		try {
			const result = await store.createNote(initialContent);
			setShowCreateNoteDialog(false);
			navigate(`/notes/${result.id}`);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create note");
		}
	};

	// Schema creation
	const handleCreateSchema = async (name: string) => {
		try {
			await schemaApi.create(workspaceId(), {
				name,
				fields: {},
				template: `# ${name}\n\n## Description\n\nEnter description here...`,
			});
			setShowCreateSchemaDialog(false);
			// Refetch schemas
			schemas.refetch?.();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create data model");
		}
	};

	// Note editor handlers
	const handleContentChange = (content: string) => {
		setEditorContent(content);
		setIsDirty(true);
		setConflictMessage(null);
	};

	const handleSave = async () => {
		const wsId = workspaceId();
		const noteId = props.noteId;
		const revisionId = currentRevisionId() || note()?.revision_id;

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
			// Refresh note list to show updated title
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
		const noteId = props.noteId;
		if (!wsId || !noteId) return;
		if (!confirm("Are you sure you want to delete this note?")) return;

		try {
			await noteApi.delete(wsId, noteId);
			store.loadNotes();
			navigate("/notes");
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

	const handleNoteSelect = (noteId: string) => {
		navigate(`/notes/${noteId}`);
	};

	const handleSchemaSelect = (schemaName: string) => {
		setSelectedSchemaName(schemaName);
	};

	return (
		<main class="flex h-screen overflow-hidden bg-gray-100">
			{/* Sidebar */}
			<aside class="w-80 flex-shrink-0 bg-white border-r flex flex-col h-full">
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

				{/* Create Button */}
				<div class="p-4 border-b">
					<button
						type="button"
						onClick={() =>
							viewMode() === "notes"
								? setShowCreateNoteDialog(true)
								: setShowCreateSchemaDialog(true)
						}
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
						{viewMode() === "notes" ? "New Note" : "New Class"}
					</button>
				</div>

				{/* Search */}
				<div class="p-4 border-b">
					<SearchBar
						onSearch={handleSearch}
						loading={isSearching()}
						resultsCount={
							viewMode() === "notes" ? filteredNotes().length : filteredSchemas().length
						}
						placeholder={
							viewMode() === "notes" ? "Search notes... (⌘K)" : "Search data models... (⌘K)"
						}
					/>
				</div>

				{/* Class Filter (Notes view only) */}
				<Show when={viewMode() === "notes" && (schemas() || []).length > 0}>
					<div class="px-4 pb-4 border-b">
						<label class="block text-xs font-medium text-gray-600 mb-1" for="class-filter">
							Filter by Class
						</label>
						<select
							id="class-filter"
							class="w-full px-3 py-2 border border-gray-300 rounded-lg bg-white text-gray-900 text-sm"
							value={classFilter()}
							onChange={(e) => setClassFilter(e.currentTarget.value)}
						>
							<option value="">All</option>
							<For each={schemas() || []}>{(s) => <option value={s.name}>{s.name}</option>}</For>
						</select>
					</div>
				</Show>

				{/* List Content */}
				<div class="flex-1 overflow-auto p-4">
					<Show when={viewMode() === "notes"}>
						<NoteList
							notes={() => filteredNotes()}
							loading={() => isSearching() || store.loading()}
							error={store.error}
							selectedNoteId={props.noteId}
							onSelect={handleNoteSelect}
						/>
					</Show>

					<Show when={viewMode() === "schemas"}>
						<SchemaListItems
							schemas={filteredSchemas()}
							selectedSchemaName={selectedSchemaName()}
							onSelect={handleSchemaSelect}
						/>
					</Show>
				</div>
			</aside>

			{/* Main Content */}
			<div class="flex-1 flex flex-col overflow-hidden">
				<Show when={viewMode() === "notes"}>
					<Show
						when={props.noteId && note()}
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
						{/* Note Editor Header */}
						<div class="bg-white border-b px-4 py-3 flex items-center justify-between">
							<div>
								<h2 class="font-semibold text-gray-800">{note()?.title || "Untitled"}</h2>
								<Show when={note()?.class}>
									<span class="text-sm text-gray-500">Class: {note()?.class}</span>
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

						{/* Note Editor */}
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
								onNoteClick={(noteId) => navigate(`/notes/${noteId}`)}
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

/**
 * Schema list items component (similar to NoteList but for schemas)
 */
interface SchemaListItemsProps {
	schemas: Schema[];
	selectedSchemaName: string | null;
	onSelect: (schemaName: string) => void;
}

function SchemaListItems(props: SchemaListItemsProps) {
	return (
		<div class="space-y-2">
			<Show
				when={props.schemas.length > 0}
				fallback={
					<div class="text-center text-gray-400 py-8">
						<svg
							class="w-12 h-12 mx-auto mb-2 opacity-50"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="1.5"
								d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"
							/>
						</svg>
						<p class="text-sm">No data models yet</p>
					</div>
				}
			>
				<For each={props.schemas}>
					{(schema) => (
						<button
							type="button"
							onClick={() => props.onSelect(schema.name)}
							class={`w-full text-left p-4 rounded-lg border cursor-pointer transition-all duration-200 ${
								props.selectedSchemaName === schema.name
									? "border-blue-500 bg-blue-50 shadow-sm ring-2 ring-blue-200"
									: "border-gray-200 bg-white hover:border-blue-300 hover:shadow-md hover:bg-gray-50"
							}`}
						>
							<div class="font-semibold text-gray-900">{schema.name}</div>
							<div class="text-xs text-gray-500 mt-1">
								{Object.keys(schema.fields).length} fields
							</div>
						</button>
					)}
				</For>
			</Show>
		</div>
	);
}
