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
				<div class="loading-indicator">
					<span class="animate-pulse">Loading notes...</span>
				</div>
			</Show>

			<Show when={error()}>
				<div class="error-message text-red-500 p-4 rounded bg-red-50">{error()}</div>
			</Show>

			<Show when={!loading() && notes().length === 0 && !error()}>
				<div class="empty-state p-8 text-center text-gray-500">
					<p>No notes yet</p>
					<p class="text-sm mt-2">Create your first note to get started</p>
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
				class={`note-item-button w-full text-left p-4 rounded-lg border cursor-pointer transition-all hover:shadow-md ${
					props.isSelected
						? "selected border-blue-500 bg-blue-50"
						: "border-gray-200 bg-white hover:border-gray-300"
				}`}
				onClick={props.onClick}
				aria-pressed={props.isSelected}
			>
				<div class="flex justify-between items-start">
					<h3 class="font-semibold text-gray-900 truncate">{props.note.title || "Untitled"}</h3>
					<Show when={props.note.class}>
						<span class="text-xs px-2 py-1 bg-gray-100 rounded text-gray-600">
							{props.note.class}
						</span>
					</Show>
				</div>

				<Show when={propertyEntries().length > 0}>
					<div class="mt-2 text-sm text-gray-600 space-y-1">
						<For each={propertyEntries()}>
							{([key, value]) => (
								<div class="flex">
									<span class="font-medium text-gray-500 mr-2">{key}:</span>
									<span class="truncate">
										{typeof value === "string" ? value : JSON.stringify(value)}
									</span>
								</div>
							)}
						</For>
					</div>
				</Show>

				<div class="mt-2 text-xs text-gray-400">Updated: {formatDate(props.note.updated_at)}</div>
			</button>
		</li>
	);
}
