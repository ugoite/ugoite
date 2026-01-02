import { For, onMount, Show, createMemo } from "solid-js";
import type { Accessor } from "solid-js";
import { createNoteStore } from "~/lib/store";
import type { NoteRecord } from "~/lib/types";

/** Props for controlled mode (passing external state) */
export interface NoteListControlledProps {
	notes: Accessor<NoteRecord[]>;
	loading: Accessor<boolean>;
	error: Accessor<string | null>;
	selectedNoteId?: string;
	onSelect?: (noteId: string) => void;
}

/** Props for standalone mode (manages its own state) */
export interface NoteListStandaloneProps {
	workspaceId: string;
	selectedNoteId?: string;
	onSelect?: (noteId: string) => void;
}

export type NoteListProps = NoteListControlledProps | NoteListStandaloneProps;

function isControlled(props: NoteListProps): props is NoteListControlledProps {
	return "notes" in props && typeof props.notes === "function";
}

export function NoteList(props: NoteListProps) {
	// Either use provided state or create internal store
	const controlled = isControlled(props);

	// For standalone mode, create internal store
	const internalStore = controlled
		? null
		: createNoteStore(() => (props as NoteListStandaloneProps).workspaceId);

	// Unified accessors
	const notes = createMemo(() =>
		controlled ? (props as NoteListControlledProps).notes() : (internalStore?.notes() ?? []),
	);
	const loading = createMemo(() =>
		controlled ? (props as NoteListControlledProps).loading() : (internalStore?.loading() ?? false),
	);
	const error = createMemo(() =>
		controlled ? (props as NoteListControlledProps).error() : (internalStore?.error() ?? null),
	);

	onMount(() => {
		// Only load if in standalone mode
		if (!controlled && internalStore) {
			internalStore.loadNotes();
		}
	});

	const handleNoteClick = (noteId: string) => {
		props.onSelect?.(noteId);
	};

	return (
		<div class="note-list-container">
			<Show when={loading()}>
				<div class="loading-indicator flex items-center justify-center p-8">
					<div class="flex flex-col items-center">
						<svg
							class="animate-spin h-8 w-8 text-blue-500 mb-2"
							xmlns="http://www.w3.org/2000/svg"
							fill="none"
							viewBox="0 0 24 24"
						>
							<circle
								class="opacity-25"
								cx="12"
								cy="12"
								r="10"
								stroke="currentColor"
								stroke-width="4"
							/>
							<path
								class="opacity-75"
								fill="currentColor"
								d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
							/>
						</svg>
						<span class="text-gray-600 text-sm">Loading notes...</span>
					</div>
				</div>
			</Show>

			<Show when={error()}>
				<div class="error-message text-red-600 p-4 rounded-lg bg-red-50 border border-red-200 flex items-start">
					<svg class="w-5 h-5 mr-2 flex-shrink-0 mt-0.5" fill="currentColor" viewBox="0 0 20 20">
						<path
							fill-rule="evenodd"
							d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z"
							clip-rule="evenodd"
						/>
					</svg>
					<span>{error()}</span>
				</div>
			</Show>

			<Show when={!loading() && notes().length === 0 && !error()}>
				<div class="empty-state p-12 text-center">
					<div class="flex flex-col items-center">
						<svg
							class="w-16 h-16 text-gray-300 mb-4"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="1.5"
								d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
							/>
						</svg>
						<p class="text-gray-600 font-medium mb-1">No notes yet</p>
						<p class="text-sm text-gray-400">Create your first note to get started</p>
					</div>
				</div>
			</Show>

			<Show when={!loading() && notes().length > 0}>
				<ul class="space-y-2">
					<For each={notes()}>
						{(note) => (
							<NoteListItem
								note={note}
								isSelected={props.selectedNoteId === note.id}
								onClick={() => handleNoteClick(note.id)}
							/>
						)}
					</For>
				</ul>
			</Show>
		</div>
	);
}

interface NoteListItemProps {
	note: NoteRecord;
	isSelected: boolean;
	onClick: () => void;
}

function NoteListItem(props: NoteListItemProps) {
	const propertyEntries = () => Object.entries(props.note.properties ?? {}).slice(0, 3);

	const formatDate = (dateStr: string) => {
		try {
			return new Date(dateStr).toLocaleDateString();
		} catch {
			return dateStr;
		}
	};

	return (
		<li data-testid="note-item">
			<button
				type="button"
				class={`note-item-button w-full text-left p-4 rounded-lg border cursor-pointer transition-all duration-200 ${
					props.isSelected
						? "selected border-blue-500 bg-blue-50 shadow-sm ring-2 ring-blue-200"
						: "border-gray-200 bg-white hover:border-blue-300 hover:shadow-md hover:bg-gray-50"
				}`}
				onClick={props.onClick}
				aria-pressed={props.isSelected}
			>
				<div class="flex justify-between items-start mb-2">
					<h3 class="font-semibold text-gray-900 truncate flex-1 pr-2">
						{props.note.title || "Untitled"}
					</h3>
					<Show when={props.note.class}>
						<span class="text-xs px-2 py-0.5 bg-blue-100 text-blue-700 rounded-full font-medium whitespace-nowrap">
							{props.note.class}
						</span>
					</Show>
				</div>

				<Show when={propertyEntries().length > 0}>
					<div class="mt-2 text-sm text-gray-600 space-y-1">
						<For each={propertyEntries()}>
							{([key, value]) => (
								<div class="flex items-baseline">
									<span class="font-medium text-gray-500 mr-2 text-xs uppercase tracking-wide">
										{key}:
									</span>
									<span class="truncate text-gray-700">
										{typeof value === "string" ? value : JSON.stringify(value)}
									</span>
								</div>
							)}
						</For>
					</div>
				</Show>

				<div class="mt-3 text-xs text-gray-400 flex items-center">
					<svg
						class="w-3 h-3 mr-1 opacity-50"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
					>
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
						/>
					</svg>
					<span>Updated {formatDate(props.note.updated_at)}</span>
				</div>
			</button>
		</li>
	);
}
