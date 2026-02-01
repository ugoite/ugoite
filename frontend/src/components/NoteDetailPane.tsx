import { createEffect, createResource, createSignal, onCleanup, Show, For } from "solid-js";
import type { Accessor } from "solid-js";
import { AttachmentUploader } from "~/components/AttachmentUploader";
import { MarkdownEditor } from "~/components/MarkdownEditor";
import { attachmentApi } from "~/lib/attachment-api";
import { noteApi, RevisionConflictError } from "~/lib/note-api";
import type { Attachment } from "~/lib/types";

export interface NoteDetailPaneProps {
	workspaceId: Accessor<string>;
	noteId: Accessor<string>;
	onDeleted: () => void;
	onAfterSave?: () => void;
}

const CLASS_VALIDATION_MARKER = "Class validation failed:";
const UNKNOWN_FIELDS_MARKER = "Unknown class fields:";

function parseClassValidationError(message: string) {
	if (!message.includes(CLASS_VALIDATION_MARKER)) return null;
	const payload = message.split(CLASS_VALIDATION_MARKER)[1]?.trim();
	if (!payload) return null;
	try {
		const parsed = JSON.parse(payload) as Array<{ field?: string; message?: string }>;
		const items = parsed
			.map((entry) => entry.message || entry.field)
			.filter((item): item is string => Boolean(item));
		return {
			title: "Class validation failed",
			items: items.length > 0 ? items : ["Please review the class requirements."],
		};
	} catch {
		return {
			title: "Class validation failed",
			items: [payload],
		};
	}
}

function parseUnknownFieldsError(message: string) {
	if (!message.includes(UNKNOWN_FIELDS_MARKER)) return null;
	const payload = message.split(UNKNOWN_FIELDS_MARKER)[1]?.trim();
	const items = payload
		? payload
				.split(",")
				.map((item) => item.trim())
				.filter(Boolean)
		: [];
	return {
		title: "Unknown class fields",
		items: items.length > 0 ? items : [payload || "Unknown fields found."],
	};
}

function parseValidationErrorMessage(message: string) {
	return parseClassValidationError(message) || parseUnknownFieldsError(message);
}

async function fetchWithTimeout<T>(
	promise: Promise<T>,
	ms = 10000,
	errorMsg = "Operation timed out",
): Promise<T> {
	let timer: ReturnType<typeof setTimeout> | undefined;
	const timeout = new Promise<never>((_, reject) => {
		timer = setTimeout(() => reject(new Error(errorMsg)), ms);
	});
	try {
		return await Promise.race([promise, timeout]);
	} finally {
		if (timer) clearTimeout(timer);
	}
}

export function NoteDetailPane(props: NoteDetailPaneProps) {
	const [attachments, setAttachments] = createSignal<Attachment[]>([]);

	const [editorContent, setEditorContent] = createSignal("");
	const [isDirty, setIsDirty] = createSignal(false);
	const [isSaving, setIsSaving] = createSignal(false);
	const [conflictMessage, setConflictMessage] = createSignal<string | null>(null);
	const [validationError, setValidationError] = createSignal<{
		title: string;
		items: string[];
	} | null>(null);
	const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(null);
	const [lastLoadedNoteId, setLastLoadedNoteId] = createSignal<string | null>(null);
	const [noteError, setNoteError] = createSignal<string | null>(null);

	const [note, { refetch: refetchNote }] = createResource(
		() => {
			const wsId = props.workspaceId();
			const noteId = props.noteId();
			return wsId && noteId ? { wsId, noteId } : null;
		},
		async (p) => {
			if (!p) return null;
			try {
				setNoteError(null);
				return await fetchWithTimeout(
					noteApi.get(p.wsId, p.noteId),
					10000,
					"Loading note timed out",
				);
			} catch (error) {
				setNoteError(error instanceof Error ? error.message : "Failed to load note");
				return null;
			}
		},
	);

	let attachmentsAbortController: AbortController | null = null;
	createEffect(() => {
		const wsId = props.workspaceId();
		if (!wsId) return;
		attachmentsAbortController?.abort();
		attachmentsAbortController = new AbortController();
		attachmentApi
			.list(wsId)
			.then((a) => {
				if (!attachmentsAbortController?.signal.aborted) setAttachments(a);
			})
			.catch(() => {
				if (!attachmentsAbortController?.signal.aborted) setAttachments([]);
			});
	});

	onCleanup(() => {
		attachmentsAbortController?.abort();
	});

	// Sync editor content when note changes (switch only)
	createEffect(() => {
		const n = note();
		if (n && n.id !== lastLoadedNoteId()) {
			setLastLoadedNoteId(n.id);
			setCurrentRevisionId(n.revision_id);
			setEditorContent(n.content ?? "");
			setIsDirty(false);
			setConflictMessage(null);
			setValidationError(null);
		}
	});

	const handleContentChange = (content: string) => {
		setEditorContent(content);
		setIsDirty(true);
		setConflictMessage(null);
		setValidationError(null);
	};

	const resolveSaveContext = () => {
		const wsId = props.workspaceId();
		const noteId = props.noteId();
		const revisionId = currentRevisionId() || note()?.revision_id;
		if (!wsId || !noteId || !revisionId) {
			return {
				ok: false,
				reason: "Cannot save: note not properly loaded. Please try refreshing.",
			};
		}
		return { ok: true, wsId, noteId, revisionId };
	};

	const handleSaveError = (error: unknown) => {
		if (error instanceof RevisionConflictError) {
			setConflictMessage(
				"This note was modified elsewhere. Please refresh to see the latest version.",
			);
			return;
		}
		const message = error instanceof Error ? error.message : "Failed to save";
		const parsed = parseValidationErrorMessage(message);
		if (parsed) {
			setValidationError(parsed);
		} else {
			setConflictMessage(message);
		}
	};

	const handleSave = async () => {
		const context = resolveSaveContext();
		if (!context.ok) {
			setConflictMessage(context.reason);
			return;
		}

		setIsSaving(true);
		setConflictMessage(null);
		setValidationError(null);

		try {
			const result = await noteApi.update(context.wsId, context.noteId, {
				markdown: editorContent(),
				parent_revision_id: context.revisionId,
			});
			setCurrentRevisionId(result.revision_id);
			setIsDirty(false);
			props.onAfterSave?.();
		} catch (e) {
			handleSaveError(e);
		} finally {
			setIsSaving(false);
		}
	};

	const handleDelete = async () => {
		const wsId = props.workspaceId();
		const noteId = props.noteId();
		if (!wsId || !noteId) return;
		if (!confirm("Are you sure you want to delete this note?")) return;

		try {
			await noteApi.delete(wsId, noteId);
			props.onDeleted();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to delete note");
		}
	};

	const handleAttachmentUpload = async (file: File): Promise<Attachment> => {
		const wsId = props.workspaceId();
		const attachment = await attachmentApi.upload(wsId, file);
		try {
			setAttachments(await attachmentApi.list(wsId));
		} catch {
			// ignore
		}
		return attachment;
	};

	return (
		<div class="flex-1 flex flex-col overflow-hidden relative h-full">
			<Show when={note.loading}>
				<div class="absolute inset-0 bg-white/80 z-50 flex items-center justify-center">
					<div class="text-center">
						<div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600 mx-auto mb-2" />
						<p class="text-gray-500 text-sm">Loading note...</p>
					</div>
				</div>
			</Show>

			<Show
				when={note()}
				fallback={
					<div class="flex-1 flex items-center justify-center text-gray-500">
						<Show
							when={noteError()}
							fallback={
								<Show when={!note.loading} fallback={<div />}>
									<p class="text-gray-700">Note not found.</p>
								</Show>
							}
						>
							<div class="text-center space-y-2">
								<p class="text-red-600">{noteError()}</p>
								<p class="text-xs text-gray-400">
									Workspace: {props.workspaceId()} / Note: {props.noteId()}
								</p>
								<button
									type="button"
									onClick={() => refetchNote()}
									class="text-blue-600 hover:underline"
								>
									Retry
								</button>
								<div class="mt-4">
									<button
										type="button"
										class="text-sm text-sky-700 hover:underline"
										onClick={props.onDeleted}
									>
										Back to notes
									</button>
								</div>
							</div>
						</Show>
					</div>
				}
			>
				{(currentNote) => (
					<div class="flex-1 flex flex-col overflow-hidden">
						<div class="bg-white border-b px-4 py-3 flex items-center justify-between">
							<div>
								<h2 class="font-semibold text-gray-800">{currentNote().title || "Untitled"}</h2>
								<Show when={currentNote().class}>
									<span class="text-sm text-gray-500">Class: {currentNote().class}</span>
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
							<Show when={validationError()}>
								{(error) => (
									<div class="mx-4 mt-4 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900">
										<p class="font-semibold">{error().title}</p>
										<ul class="mt-2 list-disc pl-5 space-y-1">
											<For each={error().items}>{(item) => <li>{item}</li>}</For>
										</ul>
									</div>
								)}
							</Show>
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
				)}
			</Show>
		</div>
	);
}
