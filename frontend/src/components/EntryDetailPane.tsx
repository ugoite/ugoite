import {
	createEffect,
	createMemo,
	createResource,
	createSignal,
	onCleanup,
	Show,
	For,
} from "solid-js";
import type { Accessor } from "solid-js";
import { isServer } from "solid-js/web";
import { AssetUploader } from "~/components/AssetUploader";
import { MarkdownEditor } from "~/components/MarkdownEditor";
import { assetApi } from "~/lib/asset-api";
import { entryApi, RevisionConflictError } from "~/lib/entry-api";
import { updateH2Section } from "~/lib/markdown";
import type { Asset, Form } from "~/lib/types";

export interface EntryDetailPaneProps {
	spaceId: Accessor<string>;
	entryId: Accessor<string>;
	forms?: Accessor<Form[]>;
	onDeleted: () => void;
	onAfterSave?: () => void;
}

const CLASS_VALIDATION_MARKER = "Form validation failed:";
const UNKNOWN_FIELDS_MARKER = "Unknown form fields:";
const BOOLEAN_VALUE_REGEX = /^(true|false|yes|no|on|off|1|0)$/i;

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

function normalizeFieldName(fieldName: string) {
	return fieldName.trim().toLowerCase();
}

function parseMarkdownH2Sections(markdown: string) {
	const lines = markdown.split(/\r?\n/);
	const sections: Array<{ title: string; content: string }> = [];
	let activeTitle: string | null = null;
	let buffer: string[] = [];

	const pushActive = () => {
		if (!activeTitle) return;
		sections.push({ title: activeTitle, content: buffer.join("\n").trim() });
	};

	for (const line of lines) {
		const heading = /^##\s+(.+?)\s*$/.exec(line);
		if (heading) {
			pushActive();
			activeTitle = heading[1];
			buffer = [];
			continue;
		}
		if (activeTitle) {
			buffer.push(line);
		}
	}

	pushActive();
	return sections;
}

function buildEditorGuidance(form: Form | null, markdown: string) {
	if (!form) {
		return {
			missingRequired: [] as string[],
			unknownSections: [] as string[],
			typeIssues: [] as string[],
		};
	}

	const sections = parseMarkdownH2Sections(markdown);
	const sectionMap = new Map<string, { title: string; content: string }>();
	for (const section of sections) {
		sectionMap.set(normalizeFieldName(section.title), section);
	}

	const formFields = Object.entries(form.fields || {});
	const knownFieldNames = new Set(formFields.map(([fieldName]) => normalizeFieldName(fieldName)));

	const missingRequired = formFields
		.filter(
			([fieldName, fieldDef]) =>
				fieldDef.required && !sectionMap.has(normalizeFieldName(fieldName)),
		)
		.map(([fieldName]) => fieldName);

	const unknownSections = sections
		.filter((section) => !knownFieldNames.has(normalizeFieldName(section.title)))
		.map((section) => section.title);

	const typeIssues: string[] = [];
	for (const [fieldName, fieldDef] of formFields) {
		const section = sectionMap.get(normalizeFieldName(fieldName));
		if (!section) continue;
		const value = section.content.trim();
		if (!value) continue;
		if (fieldDef.type === "boolean" && !BOOLEAN_VALUE_REGEX.test(value)) {
			typeIssues.push(`${fieldName}: boolean は true/false, yes/no, on/off, 1/0 を使用`);
		}
		if (
			fieldDef.type === "list" &&
			!value.includes("\n") &&
			!value.startsWith("-") &&
			value.includes(",")
		) {
			typeIssues.push(`${fieldName}: list は "- item" または1行1値を使用`);
		}
	}

	return { missingRequired, unknownSections, typeIssues };
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

	const currentForm = createMemo(() => {
		const formName = entry()?.form?.trim();
		if (!formName) return null;
		const availableForms = props.forms?.() ?? [];
		return availableForms.find((candidate) => candidate.name === formName) ?? null;
	});

	const editorGuidance = createMemo(() => buildEditorGuidance(currentForm(), editorContent()));

	let assetsAbortController: AbortController | null = null;
	createEffect(() => {
		if (isServer) return;
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

	const handleInsertMissingHeadings = () => {
		const missingRequired = editorGuidance().missingRequired;
		if (missingRequired.length === 0) return;
		let nextContent = editorContent();
		for (const fieldName of missingRequired) {
			nextContent = updateH2Section(nextContent, fieldName, "");
		}
		handleContentChange(nextContent);
	};

	type SaveContext =
		| { ok: true; wsId: string; entryId: string; revisionId: string }
		| { ok: false; reason: string };

	const resolveSaveContext = (): SaveContext => {
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
				<div class="absolute inset-0 ui-backdrop z-50 flex items-center justify-center">
					<div class="ui-card text-center">
						<div class="animate-spin rounded-full h-8 w-8 border-b-2 border-current mx-auto mb-2" />
						<p class="ui-muted text-sm">Loading entry...</p>
					</div>
				</div>
			</Show>

			<Show
				when={entry()}
				fallback={
					<div class="flex-1 flex items-center justify-center ui-muted">
						<Show
							when={entryError()}
							fallback={
								<Show when={!entry.loading} fallback={<div />}>
									<p class="ui-muted">Entry not found.</p>
								</Show>
							}
						>
							<div class="text-center space-y-2">
								<p class="ui-alert ui-alert-error text-sm">{entryError()}</p>
								<p class="text-xs ui-muted">
									Space: {props.spaceId()} / Entry: {props.entryId()}
								</p>
								<button
									type="button"
									onClick={() => refetchEntry()}
									class="ui-button ui-button-secondary text-sm"
								>
									Retry
								</button>
								<div class="mt-4">
									<button
										type="button"
										class="ui-button ui-button-secondary text-sm"
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
						<div class="ui-card flex items-center justify-between">
							<div>
								<h2 class="font-semibold">{currentEntry().title || "Untitled"}</h2>
								<Show when={currentEntry().form}>
									<span class="text-sm ui-muted">Form: {currentEntry().form}</span>
								</Show>
							</div>
							<div class="flex items-center gap-2">
								<button
									type="button"
									onClick={() => refetchEntry()}
									class="ui-button ui-button-secondary ui-button-sm inline-flex items-center gap-2 text-sm"
								>
									<svg
										class="w-4 h-4"
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
									Refresh
								</button>
								<button
									type="button"
									onClick={handleDelete}
									class="ui-button ui-button-danger ui-button-sm inline-flex items-center gap-2 text-sm"
								>
									<svg
										class="w-4 h-4"
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
									Delete
								</button>
							</div>
						</div>

						<div class="flex-1 overflow-hidden flex flex-col">
							<Show when={validationError()}>
								{(error) => (
									<div class="mx-4 mt-4 ui-alert ui-alert-warning text-sm">
										<p class="font-semibold">{error().title}</p>
										<ul class="mt-2 list-disc pl-5 space-y-1">
											<For each={error().items}>{(item) => <li>{item}</li>}</For>
										</ul>
									</div>
								)}
							</Show>
							<Show when={currentForm()}>
								{(entryForm) => (
									<div class="mx-4 mt-4 ui-alert ui-alert-warning text-sm space-y-2">
										<div class="flex items-center justify-between gap-2">
											<p class="font-semibold">
												属性は <code>## フィールド名</code> 見出しで入力
											</p>
											<Show when={editorGuidance().missingRequired.length > 0}>
												<button
													type="button"
													onClick={handleInsertMissingHeadings}
													class="ui-button ui-button-secondary ui-button-sm text-xs"
												>
													不足H2を追加
												</button>
											</Show>
										</div>
										<p class="text-xs ui-muted">
											Form: {entryForm().name} / 例: <code>## status</code>
										</p>
										<Show when={editorGuidance().missingRequired.length > 0}>
											<p class="text-xs ui-text-danger">
												必須セクション不足: {editorGuidance().missingRequired.join(", ")}
											</p>
										</Show>
										<Show when={editorGuidance().unknownSections.length > 0}>
											<p class="text-xs ui-text-danger">
												未定義セクション: {editorGuidance().unknownSections.join(", ")}
											</p>
										</Show>
										<Show when={editorGuidance().typeIssues.length > 0}>
											<ul class="text-xs ui-text-danger list-disc pl-4 space-y-1">
												<For each={editorGuidance().typeIssues}>{(item) => <li>{item}</li>}</For>
											</ul>
										</Show>
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
