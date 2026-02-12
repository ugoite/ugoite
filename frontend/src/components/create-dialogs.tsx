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

const numericFieldTypes = new Set(["integer", "long", "number", "double", "float"]);

const formatDatetimeLocal = (date: Date) => {
	const pad = (value: number) => String(value).padStart(2, "0");
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(
		date.getHours(),
	)}:${pad(date.getMinutes())}`;
};

const isLongTextField = (name: string, def: Form["fields"][string]) =>
	def.type === "markdown" || name.toLowerCase() === "sql";

const isTextareaField = (name: string, def: Form["fields"][string]) =>
	isLongTextField(name, def) || def.type === "object_list";

const resolveInputType = (def: Form["fields"][string]) => {
	if (numericFieldTypes.has(def.type)) return "number";
	if (def.type === "date") return "date";
	if (def.type === "time") return "time";
	if (def.type.startsWith("timestamp")) return "datetime-local";
	return "text";
};

const resolveTextareaPlaceholder = (def: Form["fields"][string]) =>
	def.type === "object_list" ? "[]" : "Enter value";

type FieldIssueSource = { name: string; type: string; targetForm?: string };

type FieldIssueContext = {
	availableForms?: string[];
	currentFormName?: string;
};

const getRowReferenceIssue = (field: FieldIssueSource, context?: FieldIssueContext) => {
	if (field.type !== "row_reference") return null;
	const targetForm = field.targetForm?.trim();
	if (!targetForm) return "Target form required for row_reference";
	if (isReservedMetadataForm(targetForm)) return "Target form is reserved";
	if (context?.availableForms && context.availableForms.length > 0) {
		const validTargets = new Set(
			context.availableForms.map((formName) => formName.trim().toLowerCase()),
		);
		if (context.currentFormName?.trim()) {
			validTargets.add(context.currentFormName.trim().toLowerCase());
		}
		if (!validTargets.has(targetForm.toLowerCase())) {
			return "Target form does not exist";
		}
	}
	return null;
};

const buildFieldIssues = (fields: FieldIssueSource[], context?: FieldIssueContext) => {
	const issues = new Map<number, string>();
	const seen = new Map<string, number>();
	fields.forEach((field, index) => {
		const trimmed = field.name.trim();
		if (!trimmed) return;
		if (isReservedMetadataColumn(trimmed)) {
			issues.set(index, "Reserved metadata column name");
			return;
		}
		const rowIssue = getRowReferenceIssue(field, context);
		if (rowIssue) {
			issues.set(index, rowIssue);
			return;
		}
		const normalized = trimmed.toLowerCase();
		if (seen.has(normalized)) {
			issues.set(index, "Duplicate column name");
			return;
		}
		seen.set(normalized, index);
	});
	return issues;
};

export interface CreateEntryDialogProps {
	open: boolean;
	forms: Form[];
	defaultForm?: string;
	onClose: () => void;
	onSubmit: (title: string, formName: string, requiredValues: Record<string, string>) => void;
}

/**
 * Dialog for creating a new entry with optional form selection.
 */
export function CreateEntryDialog(props: CreateEntryDialogProps) {
	const [title, setTitle] = createSignal("");
	const [selectedForm, setSelectedForm] = createSignal("");
	const [errorMessage, setErrorMessage] = createSignal<string | null>(null);
	const [requiredValues, setRequiredValues] = createSignal<Record<string, string>>({});
	let inputRef: HTMLInputElement | undefined;
	let dialogRef: HTMLDialogElement | undefined;

	const selectedFormDef = createMemo(() =>
		props.forms.find((entryForm) => entryForm.name === selectedForm()),
	);

	const requiredFields = createMemo(() => {
		const form = selectedFormDef();
		if (!form) return [] as Array<[string, Form["fields"][string]]>;
		return Object.entries(form.fields || {}).filter(([, def]) => def.required);
	});

	const buildDefaultValue = (name: string, field: Form["fields"][string]) => {
		if (name.toLowerCase() === "sql") return "SELECT * FROM entries LIMIT 50";
		switch (field.type) {
			case "integer":
			case "long":
			case "number":
			case "double":
			case "float":
				return "0";
			case "date": {
				return new Date().toISOString().slice(0, 10);
			}
			case "time": {
				return new Date().toTimeString().slice(0, 5);
			}
			case "timestamp":
			case "timestamp_tz":
			case "timestamp_ns":
			case "timestamp_tz_ns":
				return formatDatetimeLocal(new Date());
			case "object_list":
				return "[]";
			default:
				return "";
		}
	};

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

	createEffect(() => {
		if (!props.open) return;
		const form = selectedFormDef();
		if (!form) {
			setRequiredValues({});
			return;
		}
		const defaults: Record<string, string> = {};
		for (const [name, def] of Object.entries(form.fields || {})) {
			if (!def.required) continue;
			defaults[name] = buildDefaultValue(name, def);
		}
		setRequiredValues(defaults);
	});

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		const entryTitle = title().trim();
		const formName = selectedForm().trim();
		if (!entryTitle || !formName) {
			setErrorMessage("Please provide a title and select a form.");
			return;
		}
		const missing = requiredFields()
			.map(([name]) => name)
			.filter((name) => !(requiredValues()[name] || "").trim());
		if (missing.length > 0) {
			setErrorMessage(`Please fill required fields: ${missing.join(", ")}.`);
			return;
		}
		props.onSubmit(entryTitle, formName, requiredValues());
		setTitle("");
		setSelectedForm("");
	};

	return (
		<Show when={props.open}>
			<dialog
				ref={dialogRef}
				open
				class="fixed inset-0 z-50 flex items-center justify-center ui-backdrop w-full h-full"
				onClick={handleDialogClick}
				onKeyDown={(e) => {
					if (e.key === "Escape") props.onClose();
				}}
			>
				<div
					class="ui-dialog w-full max-w-md mx-4"
					role="document"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<h2 class="text-lg font-semibold mb-4">Create New Entry</h2>

					<form onSubmit={handleSubmit} class="ui-stack-sm">
						<div class="ui-field">
							<label class="ui-label" for="entry-title">
								Title
							</label>
							<input
								ref={inputRef}
								id="entry-title"
								type="text"
								value={title()}
								onInput={(e) => setTitle(e.currentTarget.value)}
								placeholder="Enter entry title..."
								class="ui-input"
								autofocus
							/>
						</div>

						<Show
							when={props.forms.length > 0}
							fallback={
								<div class="ui-card ui-card-dashed text-sm ui-muted">
									Create a form first to start writing entries.
								</div>
							}
						>
							<div class="ui-field">
								<label class="ui-label" for="entry-form">
									Form <span class="ui-text-danger">*</span>
								</label>
								<select
									id="entry-form"
									class="ui-input"
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
										<div class="ui-card mt-3">
											<p class="text-xs font-semibold ui-muted uppercase tracking-wide">Fields</p>
											<div class="mt-2 flex flex-wrap gap-2">
												<Show
													when={Object.keys(entryForm().fields || {}).length > 0}
													fallback={<span class="text-xs ui-muted">No fields defined.</span>}
												>
													<For each={Object.entries(entryForm().fields)}>
														{([name, def]) => (
															<span class="ui-pill gap-1">
																<span class="font-medium">{name}</span>
																<span class="ui-muted">({def.type})</span>
																<Show when={def.required}>
																	<span class="ui-text-danger">*</span>
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

						<Show when={requiredFields().length > 0}>
							<div class="ui-card">
								<p class="text-sm font-semibold">Required fields</p>
								<div class="ui-stack-sm mt-3">
									<For each={requiredFields()}>
										{([name, def]) => {
											const useTextarea = isTextareaField(name, def);
											const inputType = resolveInputType(def);
											const value = requiredValues()[name] ?? "";
											const handleValue = (nextValue: string) =>
												setRequiredValues((prev) => ({
													...prev,
													[name]: nextValue,
												}));
											return (
												<div class="ui-field">
													<label class="ui-label" for={`required-${name}`}>
														{name}
														<span class="ui-muted ml-2 text-xs">({def.type})</span>
													</label>
													<Show
														when={!useTextarea}
														fallback={
															<textarea
																id={`required-${name}`}
																class="ui-input ui-textarea"
																placeholder={resolveTextareaPlaceholder(def)}
																value={value}
																onInput={(e) => handleValue(e.currentTarget.value)}
															/>
														}
													>
														<input
															id={`required-${name}`}
															type={inputType}
															class="ui-input"
															value={value}
															onInput={(e) => handleValue(e.currentTarget.value)}
														/>
													</Show>
												</div>
											);
										}}
									</For>
								</div>
							</div>
						</Show>

						<Show when={errorMessage()}>
							<div class="ui-alert ui-alert-error text-sm">{errorMessage()}</div>
						</Show>

						<div class="flex justify-end gap-3 pt-2">
							<button
								type="button"
								onClick={props.onClose}
								class="ui-button ui-button-secondary text-sm"
							>
								Cancel
							</button>
							<button
								type="submit"
								disabled={!title().trim() || !selectedForm().trim() || props.forms.length === 0}
								class="ui-button ui-button-primary text-sm"
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
	formNames: string[];
	onClose: () => void;
	onSubmit: (payload: FormCreatePayload) => void;
}

/**
 * Dialog for creating a new form.
 */
export function CreateFormDialog(props: CreateFormDialogProps) {
	const [name, setName] = createSignal("");
	const [fields, setFields] = createSignal<
		Array<{ name: string; type: string; required: boolean; targetForm?: string }>
	>([]);
	let inputRef: HTMLInputElement | undefined;
	let dialogRef: HTMLDialogElement | undefined;
	const targetFormOptions = createMemo(() => {
		const options = new Set(props.formNames);
		const current = name().trim();
		if (current) options.add(current);
		return Array.from(options);
	});

	const fieldIssues = createMemo(() =>
		buildFieldIssues(fields(), {
			availableForms: props.formNames,
			currentFormName: name(),
		}),
	);

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

		const fieldRecord: Record<string, { type: string; required: boolean; target_form?: string }> =
			{};
		let template = `# ${formName}\n\n`;

		for (const f of fields()) {
			const trimmedName = f.name.trim();
			if (trimmedName) {
				const target_form = f.type === "row_reference" ? f.targetForm?.trim() : undefined;
				fieldRecord[trimmedName] = { type: f.type, required: f.required, target_form };
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
				class="fixed inset-0 z-50 flex items-center justify-center ui-backdrop w-full h-full"
				onClick={handleDialogClick}
				onKeyDown={(e) => {
					if (e.key === "Escape") props.onClose();
				}}
			>
				<div
					class="ui-dialog w-full max-w-lg mx-4 flex flex-col max-h-[90vh]"
					role="document"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<h2 class="text-lg font-semibold mb-4">Create New Form</h2>

					<form onSubmit={handleSubmit} class="ui-stack-sm flex-1 overflow-auto">
						<div class="ui-field">
							<label class="ui-label" for="form-name">
								Name
							</label>
							<input
								ref={inputRef}
								id="form-name"
								type="text"
								value={name()}
								onInput={(e) => setName(e.currentTarget.value)}
								placeholder="e.g. Meeting, Task"
								class="ui-input"
								classList={{ "ui-input-error": Boolean(nameIssue()) }}
								autofocus
							/>
							<Show when={nameIssue()}>
								<span class="text-xs ui-text-danger">{nameIssue()}</span>
							</Show>
						</div>

						<div class="ui-stack-sm">
							<div class="flex justify-between items-center">
								<span class="text-sm font-semibold">Columns</span>
								<button
									type="button"
									onClick={addField}
									class="ui-button ui-button-secondary ui-button-sm text-xs"
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
												class="ui-input ui-input-sm flex-1"
												classList={{ "ui-input-error": fieldIssues().has(i) }}
												aria-invalid={fieldIssues().has(i) || undefined}
											/>
											<select
												value={field().type}
												onChange={(e) => updateField(i, "type", e.currentTarget.value)}
												class="ui-input ui-input-sm"
											>
												<For each={props.columnTypes}>
													{(type) => <option value={type}>{type}</option>}
												</For>
											</select>
											<button
												type="button"
												onClick={() => removeField(i)}
												class="ui-button ui-button-secondary ui-button-sm"
												aria-label="Remove column"
											>
												×
											</button>
										</div>
										<Show when={field().type === "row_reference"}>
											<div class="ml-1 flex items-center gap-2">
												<span class="text-xs ui-muted">Target Form:</span>
												<input
													type="text"
													list={`row-ref-targets-${i}`}
													placeholder="e.g. Project"
													value={field().targetForm || ""}
													onInput={(e) => updateField(i, "targetForm", e.currentTarget.value)}
													class="ui-input ui-input-sm flex-1"
												/>
												<datalist id={`row-ref-targets-${i}`}>
													<For each={targetFormOptions()}>
														{(option) => <option value={option} />}
													</For>
												</datalist>
											</div>
										</Show>
										<Show when={fieldIssues().has(i)}>
											<span class="text-xs ui-text-danger">{fieldIssues().get(i)}</span>
										</Show>
									</div>
								)}
							</Index>
							<Show when={fields().length === 0}>
								<div class="ui-card text-sm ui-muted italic text-center">No columns defined</div>
							</Show>
							<div class="ui-alert ui-alert-warning text-xs space-y-1">
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

						<div class="flex justify-end gap-3 pt-4">
							<button
								type="button"
								onClick={props.onClose}
								class="ui-button ui-button-secondary text-sm"
							>
								Cancel
							</button>
							<button
								type="submit"
								disabled={!name().trim() || hasFieldIssues() || Boolean(nameIssue())}
								class="ui-button ui-button-primary text-sm"
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
	fields: Array<{
		name: string;
		type: string;
		required: boolean;
		targetForm?: string;
		defaultValue?: string;
	}>,
	existingFields: Record<string, { type: string; required: boolean; target_form?: string }>,
) {
	const fieldRecord: Record<string, { type: string; required: boolean; target_form?: string }> = {};
	const strategies: Record<string, string | null> = {};
	const currentNames = new Set<string>();

	for (const f of fields) {
		const trimmedName = f.name.trim();
		if (trimmedName) {
			const target_form = f.type === "row_reference" ? f.targetForm?.trim() : undefined;
			fieldRecord[trimmedName] = { type: f.type, required: f.required, target_form };
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
	formNames: string[];
	onClose: () => void;
	onSubmit: (payload: FormCreatePayload) => void;
}

export function EditFormDialog(props: EditFormDialogProps) {
	const [fields, setFields] = createSignal<
		Array<{
			name: string;
			type: string;
			required: boolean;
			targetForm?: string;
			defaultValue?: string;
			isNew?: boolean;
		}>
	>([]);
	let dialogRef: HTMLDialogElement | undefined;
	const targetFormOptions = createMemo(() => {
		const options = new Set(props.formNames);
		if (props.entryForm?.name) options.add(props.entryForm.name);
		return Array.from(options);
	});

	const fieldIssues = createMemo(() =>
		buildFieldIssues(fields(), {
			availableForms: props.formNames,
			currentFormName: props.entryForm?.name,
		}),
	);

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
				targetForm: def.target_form,
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
				class="fixed inset-0 z-50 flex items-center justify-center ui-backdrop w-full h-full"
				onClick={handleDialogClick}
				onKeyDown={(e) => {
					if (e.key === "Escape") props.onClose();
				}}
			>
				<div
					class="ui-dialog w-full max-w-lg mx-4 flex flex-col max-h-[90vh]"
					role="document"
					onClick={(e) => e.stopPropagation()}
					onKeyDown={(e) => e.stopPropagation()}
				>
					<h2 class="text-lg font-semibold mb-4">Edit Form: {props.entryForm?.name}</h2>
					<div class="ui-alert ui-alert-warning text-sm">
						<p>
							<strong>Warning:</strong> Removing or renaming columns will delete associated data in
							existing entries.
						</p>
					</div>

					<form onSubmit={handleSubmit} class="ui-stack-sm flex-1 overflow-auto">
						<div class="ui-stack-sm">
							<div class="flex justify-between items-center">
								<span class="text-sm font-semibold">Columns</span>
								<button
									type="button"
									onClick={addField}
									class="ui-button ui-button-secondary ui-button-sm text-xs"
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
												class="ui-input ui-input-sm flex-1"
												classList={{ "ui-input-error": fieldIssues().has(i) && field().isNew }}
												aria-invalid={fieldIssues().has(i) || undefined}
												title={!field().isNew ? "Delete and add a new column to rename" : ""}
											/>
											<select
												value={field().type}
												onChange={(e) => updateField(i, "type", e.currentTarget.value)}
												class="ui-input ui-input-sm"
											>
												<For each={props.columnTypes}>
													{(type) => <option value={type}>{type}</option>}
												</For>
											</select>
											<button
												type="button"
												onClick={() => removeField(i)}
												class="ui-button ui-button-secondary ui-button-sm"
												aria-label="Remove column"
											>
												×
											</button>
										</div>
										<Show when={field().type === "row_reference"}>
											<div class="ml-1 flex items-center gap-2">
												<span class="text-xs ui-muted">Target Form:</span>
												<input
													type="text"
													list={`row-ref-targets-edit-${i}`}
													placeholder="e.g. Project"
													value={field().targetForm || ""}
													onInput={(e) => updateField(i, "targetForm", e.currentTarget.value)}
													class="ui-input ui-input-sm flex-1"
												/>
												<datalist id={`row-ref-targets-edit-${i}`}>
													<For each={targetFormOptions()}>
														{(option) => <option value={option} />}
													</For>
												</datalist>
											</div>
										</Show>
										<Show when={fieldIssues().has(i) && field().isNew}>
											<span class="text-xs ui-text-danger">{fieldIssues().get(i)}</span>
										</Show>
										<Show when={!props.entryForm.fields[field().name] || field().isNew}>
											<div class="ml-1 flex items-center gap-2">
												<span class="text-xs ui-muted">Default Value:</span>
												<input
													type="text"
													placeholder="(Optional) e.g. Pending"
													value={field().defaultValue || ""}
													onInput={(e) => updateField(i, "defaultValue", e.currentTarget.value)}
													class="ui-input ui-input-sm flex-1"
												/>
											</div>
										</Show>
									</div>
								)}
							</Index>
							<Show when={fields().length === 0}>
								<div class="ui-card text-sm ui-muted italic text-center">No columns defined</div>
							</Show>
							<div class="ui-alert ui-alert-warning text-xs space-y-1">
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

						<div class="flex justify-end gap-3 pt-4">
							<button
								type="button"
								onClick={props.onClose}
								class="ui-button ui-button-secondary text-sm"
							>
								Cancel
							</button>
							<button
								type="submit"
								disabled={hasFieldIssues() || Boolean(nameIssue())}
								class="ui-button ui-button-primary text-sm"
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
