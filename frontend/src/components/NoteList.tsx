import { For, onMount, Show } from "solid-js";
import { createNoteStore } from "~/lib/store";
import type { NoteRecord } from "~/lib/types";

export interface NoteListProps {
	workspaceId: string;
	selectedNoteId?: string;
	onSelect?: (noteId: string) => void;
}

export function NoteList(props: NoteListProps) {
	const store = createNoteStore(() => props.workspaceId);

	onMount(() => {
		store.loadNotes();
	});

	const handleNoteClick = (noteId: string) => {
		props.onSelect?.(noteId);
	};

	return (
		<div class="note-list-container">
			<Show when={store.loading()}>
				<div class="loading-indicator">
					<span class="animate-pulse">Loading notes...</span>
				</div>
			</Show>

			<Show when={store.error()}>
				<div class="error-message text-red-500 p-4 rounded bg-red-50">
					{store.error()}
				</div>
			</Show>

			<Show when={!store.loading() && store.notes().length === 0 && !store.error()}>
				<div class="empty-state p-8 text-center text-gray-500">
					<p>No notes yet</p>
					<p class="text-sm mt-2">Create your first note to get started</p>
				</div>
			</Show>

			<Show when={!store.loading() && store.notes().length > 0}>
				<ul class="space-y-2">
					<For each={store.notes()}>
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
	const propertyEntries = () => Object.entries(props.note.properties).slice(0, 3);

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
				<h3 class="font-semibold text-gray-900 truncate">
					{props.note.title || "Untitled"}
				</h3>
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

				<div class="mt-2 text-xs text-gray-400">
					Updated: {formatDate(props.note.updated_at)}
				</div>
			</button>
		</li>
	);
}
