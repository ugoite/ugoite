import { useNavigate, useParams } from "@solidjs/router";
import { createResource, createSignal, createEffect, onMount, Show } from "solid-js";
import { AttachmentUploader } from "~/components/AttachmentUploader";
import { MarkdownEditor } from "~/components/MarkdownEditor";
import { WorkspaceSelector } from "~/components/WorkspaceSelector";
import { attachmentApi, noteApi, RevisionConflictError } from "~/lib/client";
import { createWorkspaceStore } from "~/lib/workspace-store";
import type { Attachment } from "~/lib/types";

export default function NoteDetailPage() {
	const navigate = useNavigate();
	const params = useParams<{ noteId: string }>();

	const workspaceStore = createWorkspaceStore();
	const workspaceId = () => workspaceStore.selectedWorkspaceId() || "";

	const [attachments, setAttachments] = createSignal<Attachment[]>([]);

	const [editorContent, setEditorContent] = createSignal("");
	const [isDirty, setIsDirty] = createSignal(false);
	const [isSaving, setIsSaving] = createSignal(false);
	const [conflictMessage, setConflictMessage] = createSignal<string | null>(null);
	const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(null);
	const [lastLoadedNoteId, setLastLoadedNoteId] = createSignal<string | null>(null);

	onMount(() => {
		workspaceStore.loadWorkspaces().catch(() => {
			// ignore
		});
	});

	const [note, { refetch: refetchNote }] = createResource(
		() => {
			const wsId = workspaceId();
			const noteId = params.noteId;
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

	createEffect(() => {
		const wsId = workspaceId();
		if (wsId && workspaceStore.initialized()) {
			attachmentApi
				.list(wsId)
				.then(setAttachments)
				.catch(() => setAttachments([]));
		}
	});

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
	};

	const handleWorkspaceCreate = async (name: string) => {
		try {
			const ws = await workspaceStore.createWorkspace(name);
			workspaceStore.selectWorkspace(ws.id);
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to create workspace");
		}
	};

	const handleContentChange = (content: string) => {
		setEditorContent(content);
		setIsDirty(true);
		setConflictMessage(null);
	};

	const handleSave = async () => {
		const wsId = workspaceId();
		const noteId = params.noteId;
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
		const noteId = params.noteId;
		if (!wsId || !noteId) return;
		if (!confirm("Are you sure you want to delete this note?")) return;

		try {
			await noteApi.delete(wsId, noteId);
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

	return (
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

				<div class="p-4 border-b">
					<button
						type="button"
						onClick={() => navigate("/notes")}
						class="text-sm text-blue-600 hover:underline"
					>
						← Back to Notes
					</button>
				</div>
			</aside>

			<div class="flex-1 flex flex-col overflow-hidden">
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
			</div>
		</main>
	);
}
