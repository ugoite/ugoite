import {
	createSignal,
	createEffect,
	createMemo,
	For,
	Index,
	Show,
	onMount,
	onCleanup,
} from "solid-js";
import type { Class, ClassCreatePayload } from "~/lib/types";
import { RESERVED_METADATA_COLUMNS, isReservedMetadataColumn } from "~/lib/metadata-columns";

export interface CreateNoteDialogProps {
	open: boolean;
	classes: Class[];
	defaultClass?: string;
	onClose: () => void;
	onSubmit: (title: string, className: string) => void;
}

/**
 * Dialog for creating a new note with optional class selection.
 */
export function CreateNoteDialog(props: CreateNoteDialogProps) {
	const [title, setTitle] = createSignal("");
	const [selectedClass, setSelectedClass] = createSignal("");
	const [errorMessage, setErrorMessage] = createSignal<string | null>(null);
	let inputRef: HTMLInputElement | undefined;
	let dialogRef: HTMLDialogElement | undefined;

	const selectedClassDef = createMemo(() =>
		props.classes.find((noteClass) => noteClass.name === selectedClass()),
	);

	// Handle escape key and click outside
	const handleKeyDown = (e: KeyboardEvent) => {
		if (e.key === "Escape" && props.open) {
			props.onClose();
		}
	};

	onMount(() => {
		if (typeof document !== "undefined") {
			document.addEventListener("keydown", handleKeyDown);
		}
	});

	onCleanup(() => {
		if (typeof document !== "undefined") {
			document.removeEventListener("keydown", handleKeyDown);
		}
	});

	// Reset form and focus when dialog opens
	const handleDialogClick = (e: MouseEvent) => {
		// Close if clicking on backdrop (the dialog element itself, not its content)
		if (e.target === dialogRef) {
			props.onClose();
		}
	};

	createEffect(() => {
		if (!props.open) return;
		setErrorMessage(null);
		setTitle("");
		const defaultClass = props.defaultClass?.trim();
		if (defaultClass && props.classes.some((noteClass) => noteClass.name === defaultClass)) {
			setSelectedClass(defaultClass);
		} else if (props.classes.length === 1) {
			setSelectedClass(props.classes[0].name);
		} else {
			setSelectedClass("");
		}
		setTimeout(() => inputRef?.focus(), 50);
	});

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		const noteTitle = title().trim();
		const className = selectedClass().trim();
		if (!noteTitle || !className) {
			setErrorMessage("Please provide a title and select a class.");
			return;
		}
		props.onSubmit(noteTitle, className);
		setTitle("");
		setSelectedClass("");
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

						<Show
							when={props.classes.length > 0}
							fallback={
								<div class="rounded-lg border border-dashed border-gray-200 bg-gray-50 p-4 text-sm text-gray-500">
									Create a class first to start writing notes.
								</div>
							}
						>
							<div>
								<label class="block text-sm font-medium text-gray-700 mb-1" for="note-class">
									Class <span class="text-red-500">*</span>
								</label>
								<select
									id="note-class"
									class="w-full px-3 py-2 border border-gray-300 rounded-lg bg-white text-gray-900"
									value={selectedClass()}
									onChange={(e) => {
										setSelectedClass(e.currentTarget.value);
										setErrorMessage(null);
									}}
								>
									<option value="" disabled>
										Select a class
									</option>
									<For each={props.classes}>{(s) => <option value={s.name}>{s.name}</option>}</For>
								</select>
								<Show when={selectedClassDef()}>
									{(noteClass) => (
										<div class="mt-3 rounded-lg border border-gray-200 bg-gray-50 p-3">
											<p class="text-xs font-semibold text-gray-600 uppercase tracking-wide">
												Fields
											</p>
											<div class="mt-2 flex flex-wrap gap-2">
												<Show
													when={Object.keys(noteClass().fields || {}).length > 0}
													fallback={<span class="text-xs text-gray-500">No fields defined.</span>}
												>
													<For each={Object.entries(noteClass().fields)}>
														{([name, def]) => (
															<span class="inline-flex items-center gap-1 rounded-full bg-white px-2 py-1 text-xs text-gray-700 border border-gray-200">
																<span class="font-medium">{name}</span>
																<span class="text-gray-400">({def.type})</span>
																<Show when={def.required}>
																	<span class="text-red-500">*</span>
																</Show>
															</span>
														)}
													</For>
												</Show>
											</div>
										</div>
									)}
								</Show>
							</div>
						</Show>

						<Show when={errorMessage()}>
							<div class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600">
								{errorMessage()}
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
								disabled={!title().trim() || !selectedClass().trim() || props.classes.length === 0}
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

export interface CreateClassDialogProps {
	open: boolean;
	columnTypes: string[];
	onClose: () => void;
	onSubmit: (payload: ClassCreatePayload) => void;
}

/**
 * Dialog for creating a new class.
 */
export function CreateClassDialog(props: CreateClassDialogProps) {
	const [name, setName] = createSignal("");
	const [fields, setFields] = createSignal<
		Array<{ name: string; type: string; required: boolean }>
	>([]);
	let inputRef: HTMLInputElement | undefined;
	let dialogRef: HTMLDialogElement | undefined;

	const fieldIssues = createMemo(() => {
		const issues = new Map<number, string>();
		const seen = new Map<string, number>();
		fields().forEach((field, index) => {
			const trimmed = field.name.trim();
			if (!trimmed) return;
			const normalized = trimmed.toLowerCase();
			if (isReservedMetadataColumn(trimmed)) {
				issues.set(index, "Reserved metadata column name");
				return;
			}
			if (seen.has(normalized)) {
				issues.set(index, "Duplicate column name");
				return;
			}
			seen.set(normalized, index);
		});
		return issues;
	});

	const hasFieldIssues = createMemo(() => fieldIssues().size > 0);

	// Handle escape key
	const handleKeyDown = (e: KeyboardEvent) => {
		if (e.key === "Escape" && props.open) {
			props.onClose();
		}
	};

	onMount(() => {
		if (typeof document !== "undefined") {
			document.addEventListener("keydown", handleKeyDown);
		}
	});

	onCleanup(() => {
		if (typeof document !== "undefined") {
			document.removeEventListener("keydown", handleKeyDown);
		}
	});

	const handleDialogClick = (e: MouseEvent) => {
		if (e.target === dialogRef) {
			props.onClose();
		}
	};

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		const className = name().trim();
		if (!className || hasFieldIssues()) return;

		const fieldRecord: Record<string, { type: string; required: boolean }> = {};
		let template = `# ${className}\n\n`;

		for (const f of fields()) {
			const trimmedName = f.name.trim();
			if (trimmedName) {
				fieldRecord[trimmedName] = { type: f.type, required: f.required };
				template += `## ${trimmedName}\n\n`;
			}
		}

		props.onSubmit({
			name: className,
			template,
			fields: fieldRecord,
		});
		setName("");
		setFields([]);
	};

	const addField = () => {
		setFields([...fields(), { name: "", type: "string", required: false }]);
	};

	const removeField = (index: number) => {
		const newFields = [...fields()];
		newFields.splice(index, 1);
		setFields(newFields);
	};

	const updateField = (
		index: number,
		key: keyof (typeof fields extends () => infer R ? R : never)[0],
		value: string | boolean,
	) => {
		const newFields = [...fields()];
		newFields[index] = { ...newFields[index], [key]: value } as (typeof fields extends () => infer R
			? R
			: never)[0];
		setFields(newFields);
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
					class="bg-white rounded-lg shadow-xl p-6 w-full max-w-lg mx-4 flex flex-col max-h-[90vh]"
					role="document"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<h2 class="text-lg font-semibold text-gray-900 mb-4">Create New Class</h2>

					<form onSubmit={handleSubmit} class="space-y-4 flex-1 overflow-auto">
						<div>
							<label class="block text-sm font-medium text-gray-700 mb-1" for="class-name">
								Name
							</label>
							<input
								ref={inputRef}
								id="class-name"
								type="text"
								value={name()}
								onInput={(e) => setName(e.currentTarget.value)}
								placeholder="e.g. Meeting, Task"
								class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
								autofocus
							/>
						</div>

						<div class="space-y-2">
							<div class="flex justify-between items-center">
								<span class="block text-sm font-medium text-gray-700">Columns</span>
								<button
									type="button"
									onClick={addField}
									class="text-xs text-blue-600 hover:text-blue-800 font-medium"
								>
									+ Add Column
								</button>
							</div>

							<Index each={fields()}>
								{(field, i) => (
									<div class="flex flex-col gap-1">
										<div class="flex gap-2 items-center">
											<input
												type="text"
												placeholder="Column Name"
												value={field().name}
												onInput={(e) => updateField(i, "name", e.currentTarget.value)}
												class={`flex-1 px-2 py-1 text-sm border rounded ${
													fieldIssues().has(i) ? "border-red-300 bg-red-50" : "border-gray-300"
												}`}
												aria-invalid={fieldIssues().has(i) || undefined}
											/>
											<select
												value={field().type}
												onChange={(e) => updateField(i, "type", e.currentTarget.value)}
												class="px-2 py-1 text-sm border border-gray-300 rounded bg-white"
											>
												<For each={props.columnTypes}>
													{(type) => <option value={type}>{type}</option>}
												</For>
											</select>
											<button
												type="button"
												onClick={() => removeField(i)}
												class="text-red-500 hover:text-red-700 px-2"
											>
												×
											</button>
										</div>
										<Show when={fieldIssues().has(i)}>
											<span class="text-xs text-red-600">{fieldIssues().get(i)}</span>
										</Show>
									</div>
								)}
							</Index>
							<Show when={fields().length === 0}>
								<div class="text-sm text-gray-500 italic p-2 bg-gray-50 rounded text-center">
									No columns defined
								</div>
							</Show>
							<div class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-700 space-y-1">
								<p>
									Reserved metadata columns are system-owned and cannot be used:{" "}
									{RESERVED_METADATA_COLUMNS.join(", ")}
								</p>
								<p>
									List fields accept Markdown bullets (e.g. <code>- item</code>) or one value per
									line.
								</p>
								<p>Boolean fields accept true/false, yes/no, on/off, and 1/0.</p>
							</div>
						</div>

						<div class="flex justify-end gap-3 pt-4 border-t border-gray-100">
							<button
								type="button"
								onClick={props.onClose}
								class="px-4 py-2 text-sm text-gray-600 hover:text-gray-800"
							>
								Cancel
							</button>
							<button
								type="submit"
								disabled={!name().trim() || hasFieldIssues()}
								class="px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
							>
								Create Class
							</button>
						</div>
					</form>
				</div>
			</dialog>
		</Show>
	);
}

function processFields(
	fields: Array<{ name: string; type: string; required: boolean; defaultValue?: string }>,
	existingFields: Record<string, { type: string; required: boolean }>,
) {
	const fieldRecord: Record<string, { type: string; required: boolean }> = {};
	const strategies: Record<string, string | null> = {};
	const currentNames = new Set<string>();

	for (const f of fields) {
		const trimmedName = f.name.trim();
		if (trimmedName) {
			fieldRecord[trimmedName] = { type: f.type, required: f.required };
			currentNames.add(trimmedName);
			if (!existingFields[trimmedName] && f.defaultValue) {
				strategies[trimmedName] = f.defaultValue;
			}
		}
	}

	for (const oldName of Object.keys(existingFields)) {
		if (!currentNames.has(oldName)) strategies[oldName] = null;
	}

	return { fieldRecord, strategies };
}

export interface EditClassDialogProps {
	open: boolean;
	noteClass: Class;
	columnTypes: string[];
	onClose: () => void;
	onSubmit: (payload: ClassCreatePayload) => void;
}

export function EditClassDialog(props: EditClassDialogProps) {
	const [fields, setFields] = createSignal<
		Array<{ name: string; type: string; required: boolean; defaultValue?: string; isNew?: boolean }>
	>([]);
	let dialogRef: HTMLDialogElement | undefined;

	const fieldIssues = createMemo(() => {
		const issues = new Map<number, string>();
		const seen = new Map<string, number>();
		fields().forEach((field, index) => {
			const trimmed = field.name.trim();
			if (!trimmed) return;
			const normalized = trimmed.toLowerCase();
			if (isReservedMetadataColumn(trimmed)) {
				issues.set(index, "Reserved metadata column name");
				return;
			}
			if (seen.has(normalized)) {
				issues.set(index, "Duplicate column name");
				return;
			}
			seen.set(normalized, index);
		});
		return issues;
	});

	const hasFieldIssues = createMemo(() => fieldIssues().size > 0);

	const handleKeyDown = (e: KeyboardEvent) => {
		if (e.key === "Escape" && props.open) {
			props.onClose();
		}
	};

	onMount(() => {
		if (typeof document !== "undefined") {
			document.addEventListener("keydown", handleKeyDown);
		}
	});

	onCleanup(() => {
		if (typeof document !== "undefined") {
			document.removeEventListener("keydown", handleKeyDown);
		}
	});

	createEffect(() => {
		if (props.open && props.noteClass) {
			const initialFields = Object.entries(props.noteClass.fields).map(([name, def]) => ({
				name,
				type: def.type,
				required: def.required,
				isNew: false,
			}));
			setFields(initialFields);
		}
	});

	const handleDialogClick = (e: MouseEvent) => {
		if (e.target === dialogRef) {
			props.onClose();
		}
	};

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		if (hasFieldIssues()) return;

		const { fieldRecord, strategies } = processFields(fields(), props.noteClass.fields);

		let template = `# ${props.noteClass.name}\n\n`;
		for (const f of fields()) {
			if (f.name.trim()) template += `## ${f.name.trim()}\n\n`;
		}

		props.onSubmit({
			name: props.noteClass.name,
			template,
			fields: fieldRecord,
			strategies: Object.keys(strategies).length > 0 ? strategies : undefined,
		});
	};

	const addField = () => {
		setFields([...fields(), { name: "", type: "string", required: false, isNew: true }]);
	};

	const removeField = (index: number) => {
		const newFields = [...fields()];
		newFields.splice(index, 1);
		setFields(newFields);
	};

	const updateField = (
		index: number,
		key: keyof (typeof fields extends () => infer R ? R : never)[0],
		value: string | boolean,
	) => {
		const newFields = [...fields()];
		newFields[index] = { ...newFields[index], [key]: value } as (typeof fields extends () => infer R
			? R
			: never)[0];
		setFields(newFields);
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
					class="bg-white rounded-lg shadow-xl p-6 w-full max-w-lg mx-4 flex flex-col max-h-[90vh]"
					role="document"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<h2 class="text-lg font-semibold text-gray-900 mb-4">
						Edit Class: {props.noteClass?.name}
					</h2>
					<div class="mb-4 text-sm text-gray-600 bg-yellow-50 p-2 rounded border border-yellow-200">
						<p>
							<strong>Warning:</strong> Removing or renaming columns will delete associated data in
							existing notes.
						</p>
					</div>

					<form onSubmit={handleSubmit} class="space-y-4 flex-1 overflow-auto">
						<div class="space-y-2">
							<div class="flex justify-between items-center">
								<span class="block text-sm font-medium text-gray-700">Columns</span>
								<button
									type="button"
									onClick={addField}
									class="text-xs text-blue-600 hover:text-blue-800 font-medium"
								>
									+ Add Column
								</button>
							</div>

							<Index each={fields()}>
								{(field, i) => (
									<div class="flex flex-col gap-1 border-b pb-2 mb-2 last:border-0">
										<div class="flex gap-2 items-center">
											<input
												type="text"
												placeholder="Column Name"
												disabled={!field().isNew && !!props.noteClass.fields[field().name]}
												value={field().name}
												onInput={(e) => updateField(i, "name", e.currentTarget.value)}
												class={`flex-1 px-2 py-1 text-sm border rounded disabled:bg-gray-100 disabled:text-gray-500 ${
													fieldIssues().has(i) && field().isNew
														? "border-red-300 bg-red-50"
														: "border-gray-300"
												}`}
												aria-invalid={fieldIssues().has(i) || undefined}
												title={!field().isNew ? "Delete and add a new column to rename" : ""}
											/>
											<select
												value={field().type}
												onChange={(e) => updateField(i, "type", e.currentTarget.value)}
												class="px-2 py-1 text-sm border border-gray-300 rounded bg-white"
											>
												<For each={props.columnTypes}>
													{(type) => <option value={type}>{type}</option>}
												</For>
											</select>
											<button
												type="button"
												onClick={() => removeField(i)}
												class="text-red-500 hover:text-red-700 px-2"
											>
												×
											</button>
										</div>
										<Show when={fieldIssues().has(i) && field().isNew}>
											<span class="text-xs text-red-600">{fieldIssues().get(i)}</span>
										</Show>
										<Show when={!props.noteClass.fields[field().name] || field().isNew}>
											<div class="ml-1 flex items-center gap-2">
												<span class="text-xs text-gray-500">Default Value:</span>
												<input
													type="text"
													placeholder="(Optional) e.g. Pending"
													value={field().defaultValue || ""}
													onInput={(e) => updateField(i, "defaultValue", e.currentTarget.value)}
													class="flex-1 px-2 py-0.5 text-xs border border-gray-300 rounded"
												/>
											</div>
										</Show>
									</div>
								)}
							</Index>
							<Show when={fields().length === 0}>
								<div class="text-sm text-gray-500 italic p-2 bg-gray-50 rounded text-center">
									No columns defined
								</div>
							</Show>
							<div class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-700 space-y-1">
								<p>
									Reserved metadata columns are system-owned and cannot be used:{" "}
									{RESERVED_METADATA_COLUMNS.join(", ")}
								</p>
								<p>
									List fields accept Markdown bullets (e.g. <code>- item</code>) or one value per
									line.
								</p>
								<p>Boolean fields accept true/false, yes/no, on/off, and 1/0.</p>
							</div>
						</div>

						<div class="flex justify-end gap-3 pt-4 border-t border-gray-100">
							<button
								type="button"
								onClick={props.onClose}
								class="px-4 py-2 text-sm text-gray-600 hover:text-gray-800"
							>
								Cancel
							</button>
							<button
								type="submit"
								disabled={hasFieldIssues()}
								class="px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
							>
								Save Changes
							</button>
						</div>
					</form>
				</div>
			</dialog>
		</Show>
	);
}
