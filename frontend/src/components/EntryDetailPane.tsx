import { createEffect, createResource, createSignal, onCleanup, Show, For } from "solid-js";
import type { Accessor } from "solid-js";
import { AssetUploader } from "~/components/AssetUploader";
import { MarkdownEditor } from "~/components/MarkdownEditor";
import { assetApi } from "~/lib/asset-api";
import { entryApi, RevisionConflictError } from "~/lib/entry-api";
import type { Asset } from "~/lib/types";

export interface EntryDetailPaneProps {
	spaceId: Accessor<string>;
	entryId: Accessor<string>;
	onDeleted: () => void;
	onAfterSave?: () => void;
}

const CLASS_VALIDATION_MARKER = "Form validation failed:";
const UNKNOWN_FIELDS_MARKER = "Unknown form fields:";

function parseFormValidationError(message: string) {
	if (!message.includes(CLASS_VALIDATION_MARKER)) return null;
	const payload = message.split(CLASS_VALIDATION_MARKER)[1]?.trim();
	if (!payload) return null;
	try {
		const parsed = JSON.parse(payload) as Array<{ field?: string; message?: string }>;
		const items = parsed
			.map((entry) => entry.message || entry.field)
			.filter((item): item is string => Boolean(item));
		return {
			title: "Form validation failed",
			items: items.length > 0 ? items : ["Please review the form requirements."],
		};
	} catch {
		return {
			title: "Form validation failed",
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
		title: "Unknown form fields",
		items: items.length > 0 ? items : [payload || "Unknown fields found."],
	};
}

function parseValidationErrorMessage(message: string) {
	return parseFormValidationError(message) || parseUnknownFieldsError(message);
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

export function EntryDetailPane(props: EntryDetailPaneProps) {
	const [assets, setAssets] = createSignal<Asset[]>([]);

	const [editorContent, setEditorContent] = createSignal("");
	const [isDirty, setIsDirty] = createSignal(false);
	const [isSaving, setIsSaving] = createSignal(false);
	const [conflictMessage, setConflictMessage] = createSignal<string | null>(null);
	const [validationError, setValidationError] = createSignal<{
		title: string;
		items: string[];
	} | null>(null);
	const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(null);
	const [lastLoadedEntryId, setLastLoadedEntryId] = createSignal<string | null>(null);
	const [entryError, setEntryError] = createSignal<string | null>(null);

	const [entry, { refetch: refetchEntry }] = createResource(
		() => {
			const wsId = props.spaceId();
			const entryId = props.entryId();
			return wsId && entryId ? { wsId, entryId } : null;
		},
		async (p) => {
			if (!p) return null;
			try {
				setEntryError(null);
				return await fetchWithTimeout(
					entryApi.get(p.wsId, p.entryId),
					10000,
					"Loading entry timed out",
				);
			} catch (error) {
				setEntryError(error instanceof Error ? error.message : "Failed to load entry");
				return null;
			}
		},
	);

	let assetsAbortController: AbortController | null = null;
	createEffect(() => {
		const wsId = props.spaceId();
		if (!wsId) return;
		assetsAbortController?.abort();
		assetsAbortController = new AbortController();
		assetApi
			.list(wsId)
			.then((a) => {
				if (!assetsAbortController?.signal.aborted) setAssets(a);
			})
			.catch(() => {
				if (!assetsAbortController?.signal.aborted) setAssets([]);
			});
	});

	onCleanup(() => {
		assetsAbortController?.abort();
	});

	// Sync editor content when entry changes (switch only)
	createEffect(() => {
		const n = entry();
		if (n && n.id !== lastLoadedEntryId()) {
			setLastLoadedEntryId(n.id);
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
		const wsId = props.spaceId();
		const entryId = props.entryId();
		const revisionId = currentRevisionId() || entry()?.revision_id;
		if (!wsId || !entryId || !revisionId) {
			return {
				ok: false,
				reason: "Cannot save: entry not properly loaded. Please try refreshing.",
			};
		}
		return { ok: true, wsId, entryId, revisionId };
	};

	const handleSaveError = (error: unknown) => {
		if (error instanceof RevisionConflictError) {
			setConflictMessage(
				"This entry was modified elsewhere. Please refresh to see the latest version.",
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
			const result = await entryApi.update(context.wsId, context.entryId, {
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
		const wsId = props.spaceId();
		const entryId = props.entryId();
		if (!wsId || !entryId) return;
		if (!confirm("Are you sure you want to delete this entry?")) return;

		try {
			await entryApi.delete(wsId, entryId);
			props.onDeleted();
		} catch (e) {
			alert(e instanceof Error ? e.message : "Failed to delete entry");
		}
	};

	const handleAssetUpload = async (file: File): Promise<Asset> => {
		const wsId = props.spaceId();
		const asset = await assetApi.upload(wsId, file);
		try {
			setAssets(await assetApi.list(wsId));
		} catch {
			// ignore
		}
		return asset;
	};

	return (
		<div class="flex-1 flex flex-col overflow-hidden relative h-full">
			<Show when={entry.loading}>
				<div class="absolute inset-0 bg-white/80 z-50 flex items-center justify-center">
					<div class="text-center">
						<div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600 mx-auto mb-2" />
						<p class="text-gray-500 text-sm">Loading entry...</p>
					</div>
				</div>
			</Show>

			<Show
				when={entry()}
				fallback={
					<div class="flex-1 flex items-center justify-center text-gray-500">
						<Show
							when={entryError()}
							fallback={
								<Show when={!entry.loading} fallback={<div />}>
									<p class="text-gray-700">Entry not found.</p>
								</Show>
							}
						>
							<div class="text-center space-y-2">
								<p class="text-red-600">{entryError()}</p>
								<p class="text-xs text-gray-400">
									Space: {props.spaceId()} / Entry: {props.entryId()}
								</p>
								<button
									type="button"
									onClick={() => refetchEntry()}
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
										Back to entries
									</button>
								</div>
							</div>
						</Show>
					</div>
				}
			>
				{(currentEntry) => (
					<div class="flex-1 flex flex-col overflow-hidden">
						<div class="bg-white border-b px-4 py-3 flex items-center justify-between">
							<div>
								<h2 class="font-semibold text-gray-800">{currentEntry().title || "Untitled"}</h2>
								<Show when={currentEntry().form}>
									<span class="text-sm text-gray-500">Form: {currentEntry().form}</span>
								</Show>
							</div>
							<div class="flex items-center gap-2">
								<button
									type="button"
									onClick={() => refetchEntry()}
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
									aria-label="Delete entry"
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
								mode="split"
								placeholder="Start writing in Markdown..."
							/>

							<div class="border-t p-4">
								<AssetUploader onUpload={handleAssetUpload} assets={assets()} />
							</div>
						</div>
					</div>
				)}
			</Show>
		</div>
	);
}
