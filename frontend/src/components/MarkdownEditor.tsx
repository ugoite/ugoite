import { createSignal, Show } from "solid-js";

export interface MarkdownEditorProps {
	content: string | undefined;
	onChange: (content: string) => void;
	onSave?: () => void;
	disabled?: boolean;
	isDirty?: boolean;
	isSaving?: boolean;
	conflictMessage?: string;
	showPreview?: boolean;
	placeholder?: string;
}

export function MarkdownEditor(props: MarkdownEditorProps) {
	const [isPreviewMode, setIsPreviewMode] = createSignal(false);

	const handleInput = (e: Event) => {
		const target = e.target as HTMLTextAreaElement;
		props.onChange(target.value);
	};

	const handleKeyDown = (e: KeyboardEvent) => {
		// Cmd/Ctrl + S to save
		if ((e.metaKey || e.ctrlKey) && e.key === "s") {
			e.preventDefault();
			if (props.onSave && props.isDirty) {
				props.onSave();
			}
		}
	};

	const renderMarkdown = (content: string | undefined | null) => {
		// Simple markdown rendering for preview
		// In production, use a proper markdown parser
		const safeContent = content ?? "";
		return safeContent
			.replace(/^# (.+)$/gm, '<h1 class="text-2xl font-bold mb-2">$1</h1>')
			.replace(/^## (.+)$/gm, '<h2 class="text-xl font-semibold mb-2 mt-4">$1</h2>')
			.replace(/^### (.+)$/gm, '<h3 class="text-lg font-medium mb-1 mt-3">$1</h3>')
			.replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
			.replace(/\*(.+?)\*/g, "<em>$1</em>")
			.replace(/`(.+?)`/g, '<code class="bg-gray-100 px-1 rounded">$1</code>')
			.replace(/\n/g, "<br>");
	};

	return (
		<div class="markdown-editor flex flex-col h-full">
			{/* Toolbar */}
			<div class="toolbar flex items-center justify-between p-2 border-b bg-gray-50">
				<div class="flex items-center gap-2">
					<Show when={props.showPreview}>
						<button
							type="button"
							class={`px-3 py-1 rounded text-sm ${
								isPreviewMode() ? "bg-blue-500 text-white" : "bg-white border hover:bg-gray-100"
							}`}
							onClick={() => setIsPreviewMode(!isPreviewMode())}
							aria-label={isPreviewMode() ? "Edit" : "Preview"}
						>
							{isPreviewMode() ? "Edit" : "Preview"}
						</button>
					</Show>
				</div>

				<div class="flex items-center gap-2">
					<Show when={props.isDirty}>
						<span class="text-sm text-amber-600 flex items-center gap-1">
							<span class="w-2 h-2 bg-amber-500 rounded-full" />
							Unsaved changes
						</span>
					</Show>

					<Show when={props.onSave}>
						<button
							type="button"
							class="px-4 py-1 bg-blue-500 text-white rounded hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed text-sm"
							onClick={props.onSave}
							disabled={!props.isDirty || props.isSaving || props.disabled}
							aria-label="Save"
						>
							{props.isSaving ? "Saving..." : "Save"}
						</button>
					</Show>
				</div>
			</div>

			{/* Conflict Warning */}
			<Show when={props.conflictMessage}>
				<div class="conflict-message bg-red-50 border-l-4 border-red-500 p-3 text-red-700">
					<div class="flex items-center gap-2">
						<svg class="w-5 h-5" fill="currentColor" viewBox="0 0 20 20">
							<path
								fill-rule="evenodd"
								d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z"
								clip-rule="evenodd"
							/>
						</svg>
						<span>{props.conflictMessage}</span>
					</div>
				</div>
			</Show>

			{/* Editor / Preview */}
			<div class="flex-1 overflow-hidden">
				<Show
					when={isPreviewMode()}
					fallback={
						<textarea
							class="w-full h-full p-4 resize-none font-mono text-sm border-0 focus:outline-none focus:ring-0"
						value={props.content ?? ""}
							onInput={handleInput}
							onKeyDown={handleKeyDown}
							disabled={props.disabled}
							placeholder={props.placeholder || "Start writing in Markdown..."}
							spellcheck={false}
						/>
					}
				>
					<div
						class="preview p-4 prose prose-sm max-w-none overflow-auto h-full"
						innerHTML={renderMarkdown(props.content ?? "")}
					/>
				</Show>
			</div>
		</div>
	);
}
