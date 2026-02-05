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
import type { Form, FormCreatePayload } from "~/lib/types";
import { RESERVED_METADATA_COLUMNS, isReservedMetadataColumn } from "~/lib/metadata-columns";
import { RESERVED_METADATA_CLASSES, isReservedMetadataForm } from "~/lib/metadata-forms";

export interface CreateEntryDialogProps {
	open: boolean;
	forms: Form[];
	defaultForm?: string;
	onClose: () => void;
	onSubmit: (title: string, formName: string) => void;
}

/**
 * Dialog for creating a new entry with optional form selection.
 */
export function CreateEntryDialog(props: CreateEntryDialogProps) {
	const [title, setTitle] = createSignal("");
	const [selectedForm, setSelectedForm] = createSignal("");
	const [errorMessage, setErrorMessage] = createSignal<string | null>(null);
	let inputRef: HTMLInputElement | undefined;
	let dialogRef: HTMLDialogElement | undefined;

	const selectedFormDef = createMemo(() =>
		props.forms.find((entryForm) => entryForm.name === selectedForm()),
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
		const defaultForm = props.defaultForm?.trim();
		if (defaultForm && props.forms.some((entryForm) => entryForm.name === defaultForm)) {
			setSelectedForm(defaultForm);
		} else if (props.forms.length === 1) {
			setSelectedForm(props.forms[0].name);
		} else {
			setSelectedForm("");
		}
		setTimeout(() => inputRef?.focus(), 50);
	});

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		const entryTitle = title().trim();
		const formName = selectedForm().trim();
		if (!entryTitle || !formName) {
			setErrorMessage("Please provide a title and select a form.");
			return;
		}
		props.onSubmit(entryTitle, formName);
		setTitle("");
		setSelectedForm("");
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
					<h2 class="text-lg font-semibold text-gray-900 mb-4">Create New Entry</h2>

					<form onSubmit={handleSubmit} class="space-y-4">
						<div>
							<label class="block text-sm font-medium text-gray-700 mb-1" for="entry-title">
								Title
							</label>
							<input
								ref={inputRef}
								id="entry-title"
								type="text"
								value={title()}
								onInput={(e) => setTitle(e.currentTarget.value)}
								placeholder="Enter entry title..."
								class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
								autofocus
							/>
						</div>

						<Show
							when={props.forms.length > 0}
							fallback={
								<div class="rounded-lg border border-dashed border-gray-200 bg-gray-50 p-4 text-sm text-gray-500">
									Create a form first to start writing entries.
								</div>
							}
						>
							<div>
								<label class="block text-sm font-medium text-gray-700 mb-1" for="entry-form">
									Form <span class="text-red-500">*</span>
								</label>
								<select
									id="entry-form"
									class="w-full px-3 py-2 border border-gray-300 rounded-lg bg-white text-gray-900"
									value={selectedForm()}
									onChange={(e) => {
										setSelectedForm(e.currentTarget.value);
										setErrorMessage(null);
									}}
								>
									<option value="" disabled>
										Select a form
									</option>
									<For each={props.forms}>{(s) => <option value={s.name}>{s.name}</option>}</For>
								</select>
								<Show when={selectedFormDef()}>
									{(entryForm) => (
										<div class="mt-3 rounded-lg border border-gray-200 bg-gray-50 p-3">
											<p class="text-xs font-semibold text-gray-600 uppercase tracking-wide">
												Fields
											</p>
											<div class="mt-2 flex flex-wrap gap-2">
												<Show
													when={Object.keys(entryForm().fields || {}).length > 0}
													fallback={<span class="text-xs text-gray-500">No fields defined.</span>}
												>
													<For each={Object.entries(entryForm().fields)}>
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
								disabled={!title().trim() || !selectedForm().trim() || props.forms.length === 0}
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

export interface CreateFormDialogProps {
	open: boolean;
	columnTypes: string[];
	onClose: () => void;
	onSubmit: (payload: FormCreatePayload) => void;
}

/**
 * Dialog for creating a new form.
 */
export function CreateFormDialog(props: CreateFormDialogProps) {
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

	const nameIssue = createMemo(() =>
		isReservedMetadataForm(name()) ? "Reserved metadata form name" : "",
	);

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
		const formName = name().trim();
		if (!formName || hasFieldIssues() || nameIssue()) return;

		const fieldRecord: Record<string, { type: string; required: boolean }> = {};
		let template = `# ${formName}\n\n`;

		for (const f of fields()) {
			const trimmedName = f.name.trim();
			if (trimmedName) {
				fieldRecord[trimmedName] = { type: f.type, required: f.required };
				template += `## ${trimmedName}\n\n`;
			}
		}

		props.onSubmit({
			name: formName,
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
					<h2 class="text-lg font-semibold text-gray-900 mb-4">Create New Form</h2>

					<form onSubmit={handleSubmit} class="space-y-4 flex-1 overflow-auto">
						<div>
							<label class="block text-sm font-medium text-gray-700 mb-1" for="form-name">
								Name
							</label>
							<input
								ref={inputRef}
								id="form-name"
								type="text"
								value={name()}
								onInput={(e) => setName(e.currentTarget.value)}
								placeholder="e.g. Meeting, Task"
								class={`w-full px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 ${
									nameIssue() ? "border-red-300 bg-red-50" : "border-gray-300"
								}`}
								autofocus
							/>
							<Show when={nameIssue()}>
								<span class="text-xs text-red-600">{nameIssue()}</span>
							</Show>
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
									Reserved metadata forms cannot be used: {RESERVED_METADATA_CLASSES.join(", ")}
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
								disabled={!name().trim() || hasFieldIssues() || Boolean(nameIssue())}
								class="px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
							>
								Create Form
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

export interface EditFormDialogProps {
	open: boolean;
	entryForm: Form;
	columnTypes: string[];
	onClose: () => void;
	onSubmit: (payload: FormCreatePayload) => void;
}

export function EditFormDialog(props: EditFormDialogProps) {
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
	const nameIssue = createMemo(() =>
		props.entryForm ? isReservedMetadataForm(props.entryForm.name) : false,
	);

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
		if (props.open && props.entryForm) {
			const initialFields = Object.entries(props.entryForm.fields).map(([name, def]) => ({
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
		if (hasFieldIssues() || nameIssue()) return;

		const { fieldRecord, strategies } = processFields(fields(), props.entryForm.fields);

		let template = `# ${props.entryForm.name}\n\n`;
		for (const f of fields()) {
			if (f.name.trim()) template += `## ${f.name.trim()}\n\n`;
		}

		props.onSubmit({
			name: props.entryForm.name,
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
						Edit Form: {props.entryForm?.name}
					</h2>
					<div class="mb-4 text-sm text-gray-600 bg-yellow-50 p-2 rounded border border-yellow-200">
						<p>
							<strong>Warning:</strong> Removing or renaming columns will delete associated data in
							existing entries.
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
												disabled={!field().isNew && !!props.entryForm.fields[field().name]}
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
										<Show when={!props.entryForm.fields[field().name] || field().isNew}>
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
								<Show when={nameIssue()}>
									<p>
										Reserved metadata forms cannot be edited: {RESERVED_METADATA_CLASSES.join(", ")}
									</p>
								</Show>
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
								disabled={hasFieldIssues() || Boolean(nameIssue())}
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
