import { createSignal, For, Show, onMount, onCleanup } from "solid-js";
import type { Schema } from "~/lib/types";

export interface CreateNoteDialogProps {
	open: boolean;
	schemas: Schema[];
	onClose: () => void;
	onSubmit: (title: string, className: string) => void;
}

/**
 * Dialog for creating a new note with optional class selection.
 */
export function CreateNoteDialog(props: CreateNoteDialogProps) {
	const [title, setTitle] = createSignal("");
	const [selectedClass, setSelectedClass] = createSignal("");
	let inputRef: HTMLInputElement | undefined;
	let dialogRef: HTMLDialogElement | undefined;

	// Handle escape key and click outside
	const handleKeyDown = (e: KeyboardEvent) => {
		if (e.key === "Escape" && props.open) {
			props.onClose();
		}
	};

	onMount(() => {
		document.addEventListener("keydown", handleKeyDown);
	});

	onCleanup(() => {
		document.removeEventListener("keydown", handleKeyDown);
	});

	// Reset form and focus when dialog opens
	const handleDialogClick = (e: MouseEvent) => {
		// Close if clicking on backdrop (the dialog element itself, not its content)
		if (e.target === dialogRef) {
			props.onClose();
		}
	};

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		const noteTitle = title().trim();
		if (noteTitle) {
			props.onSubmit(noteTitle, selectedClass());
			setTitle("");
			setSelectedClass("");
		}
	};

	// Focus input when dialog opens
	onMount(() => {
		if (props.open && inputRef) {
			setTimeout(() => inputRef?.focus(), 50);
		}
	});

	return (
		<Show when={props.open}>
			<dialog
				ref={dialogRef}
				open
				class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 w-full h-full"
				onClick={handleDialogClick}
				onKeyDown={(e) => {
					if (e.key === "Escape") props.onClose();
				}}
			>
				<div
					class="bg-white rounded-lg shadow-xl p-6 w-full max-w-md mx-4"
					role="document"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<h2 class="text-lg font-semibold text-gray-900 mb-4">Create New Note</h2>

					<form onSubmit={handleSubmit} class="space-y-4">
						<div>
							<label class="block text-sm font-medium text-gray-700 mb-1" for="note-title">
								Title
							</label>
							<input
								ref={inputRef}
								id="note-title"
								type="text"
								value={title()}
								onInput={(e) => setTitle(e.currentTarget.value)}
								placeholder="Enter note title..."
								class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
								autofocus
							/>
						</div>

						<Show when={props.schemas.length > 0}>
							<div>
								<label class="block text-sm font-medium text-gray-700 mb-1" for="note-class">
									Class (optional)
								</label>
								<select
									id="note-class"
									class="w-full px-3 py-2 border border-gray-300 rounded-lg bg-white text-gray-900"
									value={selectedClass()}
									onChange={(e) => setSelectedClass(e.currentTarget.value)}
								>
									<option value="">(none)</option>
									<For each={props.schemas}>{(s) => <option value={s.name}>{s.name}</option>}</For>
								</select>
							</div>
						</Show>

						<div class="flex justify-end gap-3 pt-2">
							<button
								type="button"
								onClick={props.onClose}
								class="px-4 py-2 text-sm text-gray-600 hover:text-gray-800"
							>
								Cancel
							</button>
							<button
								type="submit"
								disabled={!title().trim()}
								class="px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
							>
								Create
							</button>
						</div>
					</form>
				</div>
			</dialog>
		</Show>
	);
}

export interface CreateSchemaDialogProps {
	open: boolean;
	onClose: () => void;
	onSubmit: (name: string) => void;
}

/**
 * Dialog for creating a new schema/data model.
 */
export function CreateSchemaDialog(props: CreateSchemaDialogProps) {
	const [name, setName] = createSignal("");
	let inputRef: HTMLInputElement | undefined;
	let dialogRef: HTMLDialogElement | undefined;

	// Handle escape key
	const handleKeyDown = (e: KeyboardEvent) => {
		if (e.key === "Escape" && props.open) {
			props.onClose();
		}
	};

	onMount(() => {
		document.addEventListener("keydown", handleKeyDown);
	});

	onCleanup(() => {
		document.removeEventListener("keydown", handleKeyDown);
	});

	const handleDialogClick = (e: MouseEvent) => {
		if (e.target === dialogRef) {
			props.onClose();
		}
	};

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		const schemaName = name().trim();
		if (schemaName) {
			props.onSubmit(schemaName);
			setName("");
		}
	};

	return (
		<Show when={props.open}>
			<dialog
				ref={dialogRef}
				open
				class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 w-full h-full"
				onClick={handleDialogClick}
				onKeyDown={(e) => {
					if (e.key === "Escape") props.onClose();
				}}
			>
				<div
					class="bg-white rounded-lg shadow-xl p-6 w-full max-w-md mx-4"
					role="document"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<h2 class="text-lg font-semibold text-gray-900 mb-4">Create New Data Model</h2>

					<form onSubmit={handleSubmit} class="space-y-4">
						<div>
							<label class="block text-sm font-medium text-gray-700 mb-1" for="schema-name">
								Name
							</label>
							<input
								ref={inputRef}
								id="schema-name"
								type="text"
								value={name()}
								onInput={(e) => setName(e.currentTarget.value)}
								placeholder="Enter data model name..."
								class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
								autofocus
							/>
						</div>

						<div class="flex justify-end gap-3 pt-2">
							<button
								type="button"
								onClick={props.onClose}
								class="px-4 py-2 text-sm text-gray-600 hover:text-gray-800"
							>
								Cancel
							</button>
							<button
								type="submit"
								disabled={!name().trim()}
								class="px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
							>
								Create
							</button>
						</div>
					</form>
				</div>
			</dialog>
		</Show>
	);
}
