import {
	For,
	createResource,
	createSignal,
	createMemo,
	untrack,
	Show,
	onMount,
	onCleanup,
} from "solid-js";
import type { Form, EntryRecord } from "~/lib/types";
import { entryApi } from "~/lib/entry-api";
import { searchApi } from "~/lib/search-api";
import { replaceFirstH1, ensureFormFrontmatter, updateH2Section } from "~/lib/markdown";

interface FormTableProps {
	spaceId: string;
	entryForm: Form;
	onEntryClick: (entryId: string) => void;
}

type SortDirection = "asc" | "desc" | null;

/** Helper to filter entries by title or properties */
function filterEntries(entries: EntryRecord[], fields: string[], query: string) {
	if (!query) return entries;
	const text = query.toLowerCase();
	return entries.filter((entry) => {
		const title = (entry.title || "").toLowerCase();
		if (title.includes(text)) return true;
		for (const field of fields) {
			const val = String(entry.properties?.[field] ?? "").toLowerCase();
			if (val.includes(text)) return true;
		}
		return false;
	});
}

/** Helper for column-specific filtering */
function applyColumnFilters(entries: EntryRecord[], filters: Record<string, string>) {
	const activeFilters = Object.entries(filters).filter(([_, val]) => !!val);
	if (activeFilters.length === 0) return entries;

	return entries.filter((entry) => {
		for (const [field, filter] of activeFilters) {
			const filterLower = filter.toLowerCase();
			let val = "";
			if (field === "title") val = entry.title || "";
			else if (field === "updated_at") val = new Date(entry.updated_at).toLocaleDateString();
			else val = String(entry.properties?.[field] ?? "");

			if (!val.toLowerCase().includes(filterLower)) return false;
		}
		return true;
	});
}

function sortEntries(entries: EntryRecord[], field: string, direction: SortDirection) {
	if (!field || !direction) return entries;
	return [...entries].sort((a, b) => {
		let valA: string | number | unknown;
		let valB: string | number | unknown;

		if (field === "title") {
			valA = a.title || "";
			valB = b.title || "";
		} else if (field === "updated_at") {
			valA = a.updated_at;
			valB = b.updated_at;
		} else {
			valA = a.properties?.[field] ?? "";
			valB = b.properties?.[field] ?? "";
		}

		if (valA < valB) return direction === "asc" ? -1 : 1;
		if (valA > valB) return direction === "asc" ? 1 : -1;
		return 0;
	});
}

function SortIcon(props: { active: boolean; direction: SortDirection }) {
	if (!props.active || !props.direction) {
		return (
			<svg
				class="w-4 h-4 ui-muted opacity-50"
				fill="none"
				stroke="currentColor"
				viewBox="0 0 24 24"
			>
				<title>Sort</title>
				<path
					stroke-linecap="round"
					stroke-linejoin="round"
					stroke-width="2"
					d="M7 16V4m0 0L3 8m4-4l4 4m6 0v12m0 0l4-4m-4 4l-4-4"
				/>
			</svg>
		);
	}
	if (props.direction === "asc") {
		return (
			<svg class="w-4 h-4 ui-accent-text" fill="none" stroke="currentColor" viewBox="0 0 24 24">
				<title>Sorted Ascending</title>
				<path
					stroke-linecap="round"
					stroke-linejoin="round"
					stroke-width="2"
					d="M3 4h13M3 8h9m-9 4h6m4 0l4-4m0 0l4 4m-4-4v12"
				/>
			</svg>
		);
	}
	return (
		<svg class="w-4 h-4 ui-accent-text" fill="none" stroke="currentColor" viewBox="0 0 24 24">
			<title>Sorted Descending</title>
			<path
				stroke-linecap="round"
				stroke-linejoin="round"
				stroke-width="2"
				d="M3 4h13M3 8h9m-9 4h5m1 5v6m0 0l4-4m-4 4l-4-4"
			/>
		</svg>
	);
}

/** Helper to format a single entry as a CSV row */
function formatCsvRow(entry: EntryRecord, headers: string[]) {
	return headers
		.map((field) => {
			let val = "";
			if (field === "title") val = entry.title || "";
			else if (field === "updated_at") {
				try {
					val = new Date(entry.updated_at).toISOString();
				} catch {
					val = entry.updated_at;
				}
			} else {
				val = String(entry.properties?.[field] ?? "");
			}
			return `"${val.replace(/"/g, '""')}"`;
		})
		.join(",");
}

export function FormTable(props: FormTableProps) {
	// State for filtering and sorting
	const [globalFilter, setGlobalFilter] = createSignal("");
	const [sortField, setSortField] = createSignal<string | null>(null);
	const [sortDirection, setSortDirection] = createSignal<SortDirection>(null);
	const [columnFilters, setColumnFilters] = createSignal<Record<string, string>>({});
	const [isEditMode, setIsEditMode] = createSignal(false);
	const [editingCell, setEditingCell] = createSignal<{ id: string; field: string } | null>(null);

	const [entries, { refetch, mutate }] = createResource(
		() => {
			if (!props.spaceId || !props.entryForm?.name) return false;
			return { id: props.spaceId, formName: props.entryForm.name };
		},
		async ({ id, formName }) => {
			return await searchApi.query(id, { form: formName });
		},
	);

	const fields = createMemo(() =>
		props.entryForm?.fields ? Object.keys(props.entryForm.fields) : [],
	);

	const processedEntries = createMemo(() => {
		const currentEntries = entries();
		if (!currentEntries) return [] as EntryRecord[];

		// 1. Global Filter
		let result = filterEntries([...currentEntries], fields(), globalFilter());

		// 2. Column Filters
		result = applyColumnFilters(result, columnFilters());

		// 3. Sorting
		return sortEntries(result, sortField() || "", sortDirection());
	});

	const handleHeaderClick = (field: string) => {
		if (sortField() === field) {
			if (sortDirection() === "asc") setSortDirection("desc");
			else setSortDirection(null);
		} else {
			setSortField(field);
			setSortDirection("asc");
		}
	};

	const updateColumnFilter = (field: string, value: string) => {
		setColumnFilters((prev) => ({ ...prev, [field]: value }));
	};

	const downloadCSV = () => {
		// Use untrack and try-catch for robustness in handler
		try {
			const { data, fieldNames, formName } = untrack(() => ({
				data: processedEntries(),
				fieldNames: fields(),
				formName: props.entryForm?.name || "export",
			}));

			const headers = ["title", ...fieldNames, "updated_at"];
			const csvContent = [headers.join(","), ...data.map((n) => formatCsvRow(n, headers))].join(
				"\n",
			);

			const blob = new Blob([csvContent], { type: "text/csv;charset=utf-8;" });
			const url = URL.createObjectURL(blob);
			const link = document.createElement("a");
			link.setAttribute("href", url);
			link.setAttribute("download", `${formName}_export.csv`);
			document.body.appendChild(link);
			link.click();
			document.body.removeChild(link);
			// Clean up the URL object after some time
			setTimeout(() => URL.revokeObjectURL(url), 100);
		} catch (err) {
			// biome-ignore lint/suspicious/noConsole: error reporting
			console.error("CSV Export failed:", err);
			alert("Failed to export CSV. Please check the console for details.");
		}
	};

	const handleAddRow = async () => {
		try {
			let content = props.entryForm.template || `# New ${props.entryForm.name}\n`;
			content = ensureFormFrontmatter(content, props.entryForm.name);

			await entryApi.create(props.spaceId, {
				content,
			});
			refetch();
		} catch (err) {
			// biome-ignore lint/suspicious/noConsole: error logging
			console.error("Failed to add row", err);
			alert(`Failed to add row: ${err instanceof Error ? err.message : String(err)}`);
		}
	};

	const handleCellUpdate = async (entryId: string, field: string, value: string) => {
		try {
			// Fetch full entry to get content and revision_id
			const entry = await entryApi.get(props.spaceId, entryId);
			let updatedMarkdown = entry.content;

			if (field === "title") {
				if (entry.title === value) return;
				updatedMarkdown = replaceFirstH1(updatedMarkdown, value);
			} else {
				if (String(entry.properties?.[field] ?? "") === value) return;
				updatedMarkdown = updateH2Section(updatedMarkdown, field, value);
			}

			const updatedEntry = await entryApi.update(props.spaceId, entryId, {
				markdown: updatedMarkdown,
				parent_revision_id: entry.revision_id,
			});

			// Partial rendering: update local state with server response
			mutate((prev) => {
				if (!prev) return prev;
				return prev.map((n) => {
					if (n.id === entryId) {
						const next = { ...n };
						if (field === "title") {
							next.title = value;
						} else {
							next.properties = { ...(n.properties || {}), [field]: value };
						}
						next.updated_at = updatedEntry.updated_at;
						return next;
					}
					return n;
				});
			});
		} catch (err) {
			// biome-ignore lint/suspicious/noConsole: error logging
			console.error("Update failed", err);
			alert(`Update failed: ${err instanceof Error ? err.message : String(err)}`);
		}
	};

	// Determine if a cell is being edited
	const isCellEditing = (id: string, field: string) =>
		isEditMode() && editingCell()?.id === id && editingCell()?.field === field;

	// --- Drag Selection Logic ---
	const [selection, setSelection] = createSignal<{
		start: { r: number; c: number } | null;
		end: { r: number; c: number } | null;
	}>({ start: null, end: null });
	const [isSelecting, setIsSelecting] = createSignal(false);

	onMount(() => {
		document.addEventListener("mouseup", handleGlobalMouseUp);
		document.addEventListener("keydown", handleGlobalKeyDown);
	});

	onCleanup(() => {
		document.removeEventListener("mouseup", handleGlobalMouseUp);
		document.removeEventListener("keydown", handleGlobalKeyDown);
	});

	const handleGlobalMouseUp = () => setIsSelecting(false);

	const getRowData = (entry: EntryRecord, currentFields: string[], c1: number, c2: number) => {
		const rowData = [];
		// Col 0: Title
		if (c1 <= 0 && c2 >= 0) rowData.push(entry.title || "");

		// Cols 1..N: Fields
		for (let i = 0; i < currentFields.length; i++) {
			const colIdx = i + 1;
			if (colIdx >= c1 && colIdx <= c2) {
				rowData.push(String(entry.properties?.[currentFields[i]] ?? ""));
			}
		}

		// Col N+1: Updated
		const lastCol = currentFields.length + 1;
		if (c1 <= lastCol && c2 >= lastCol) {
			rowData.push(new Date(entry.updated_at).toLocaleDateString());
		}
		return rowData.join("\t");
	};

	const copySelection = async () => {
		const sel = selection();
		if (!sel.start || !sel.end || editingCell()) return;

		const r1 = Math.min(sel.start.r, sel.end.r);
		const r2 = Math.max(sel.start.r, sel.end.r);
		const c1 = Math.min(sel.start.c, sel.end.c);
		const c2 = Math.max(sel.start.c, sel.end.c);

		const currentEntries = processedEntries();
		const currentFields = fields();

		const rowsData = [];
		for (let r = r1; r <= r2; r++) {
			const entry = currentEntries[r];
			if (!entry) continue;
			rowsData.push(getRowData(entry, currentFields, c1, c2));
		}

		try {
			await navigator.clipboard.writeText(rowsData.join("\n"));
		} catch (err) {
			// biome-ignore lint/suspicious/noConsole: debugging
			console.error("Failed to copy", err);
		}
	};

	const handleGlobalKeyDown = async (e: KeyboardEvent) => {
		if ((e.ctrlKey || e.metaKey) && e.key === "c") {
			// If focus is in an input or textarea, let the default copy behavior handle it
			const active = document.activeElement;
			if (active && (active.tagName === "INPUT" || active.tagName === "TEXTAREA")) {
				return;
			}
			e.preventDefault();
			await copySelection();
		}
	};

	const handleCellMouseDown = (r: number, c: number) => {
		// Only set start/end, don't set isSelecting yet to allow text selection
		setSelection({ start: { r, c }, end: { r, c } });
	};

	const handleCellMouseEnter = (e: MouseEvent, r: number, c: number) => {
		if (isSelecting()) {
			setSelection((prev) => ({ ...prev, end: { r, c } }));
		} else if (selection().start && e.buttons === 1) {
			// Start drag selection if moving between cells with button down
			setIsSelecting(true);
			setSelection((prev) => ({ ...prev, end: { r, c } }));
		}
	};

	const isSelected = (r: number, c: number) => {
		const sel = selection();
		if (!sel.start || !sel.end) return false;
		const r1 = Math.min(sel.start.r, sel.end.r);
		const r2 = Math.max(sel.start.r, sel.end.r);
		const c1 = Math.min(sel.start.c, sel.end.c);
		const c2 = Math.max(sel.start.c, sel.end.c);
		return r >= r1 && r <= r2 && c >= c1 && c <= c2;
	};

	return (
		<div class={`flex-1 h-full overflow-auto ui-surface ${isSelecting() ? "select-none" : ""}`}>
			<div class="p-6">
				<div class="mb-6 flex justify-between items-start">
					<div>
						<p class="ui-muted text-sm">
							{entries.loading && !entries()
								? "Loading..."
								: `${processedEntries().length} records found`}
						</p>
					</div>
					<div class="flex gap-2">
						<button
							type="button"
							onClick={downloadCSV}
							class="ui-button ui-button-secondary text-sm flex items-center gap-2"
						>
							<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<title>Download CSV</title>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"
								/>
							</svg>
							Export CSV
						</button>
						<button
							type="button"
							onClick={() => setIsEditMode(!isEditMode())}
							class={`ui-button text-sm flex items-center gap-2 ${
								isEditMode() ? "ui-button-primary" : "ui-button-secondary"
							}`}
							title={isEditMode() ? "Disable Editing" : "Enable Editing"}
						>
							{isEditMode() ? (
								<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
									<title>Unlocked</title>
									<path
										stroke-linecap="round"
										stroke-linejoin="round"
										stroke-width="2"
										d="M13.5 10.5V6.75a4.5 4.5 0 1 1 9 0v3.75M3.75 21.75h10.5a2.25 2.25 0 0 0 2.25-2.25v-6.75a2.25 2.25 0 0 0-2.25-2.25H3.75a2.25 2.25 0 0 0-2.25 2.25v6.75a2.25 2.25 0 0 0 2.25 2.25Z"
									/>
								</svg>
							) : (
								<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
									<title>Locked</title>
									<path
										stroke-linecap="round"
										stroke-linejoin="round"
										stroke-width="2"
										d="M16.5 10.5V6.75a4.5 4.5 0 1 0-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 0 0 2.25-2.25v-6.75a2.25 2.25 0 0 0-2.25-2.25H6.75a2.25 2.25 0 0 0-2.25 2.25v6.75a2.25 2.25 0 0 0 2.25 2.25Z"
									/>
								</svg>
							)}
							{isEditMode() ? "Editable" : "Locked"}
						</button>
						<Show when={isEditMode()}>
							<button
								type="button"
								onClick={handleAddRow}
								class="ui-button ui-button-primary text-sm flex items-center gap-2"
							>
								<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
									<path
										stroke-linecap="round"
										stroke-linejoin="round"
										stroke-width="2"
										d="M12 4v16m8-8H4"
									/>
								</svg>
								Add Row
							</button>
						</Show>
					</div>
				</div>

				<div class="mb-4">
					<input
						type="text"
						placeholder="Global Search..."
						class="ui-input w-full max-w-md"
						value={globalFilter()}
						onInput={(e) => setGlobalFilter(e.currentTarget.value)}
					/>
				</div>

				<div class="ui-table-wrapper overflow-x-auto">
					<table class="ui-table">
						<thead class="ui-table-head">
							<tr>
								<th
									scope="col"
									class="ui-table-header-cell w-10 sticky top-0 z-10"
									aria-label="Actions"
								>
									<span class="sr-only">Actions</span>
								</th>
								<th scope="col" class="ui-table-header-cell sticky top-0 z-10">
									<div class="flex flex-col gap-2">
										<button
											type="button"
											class="ui-table-header-button select-none"
											onClick={() => handleHeaderClick("title")}
										>
											Title
											<SortIcon active={sortField() === "title"} direction={sortDirection()} />
										</button>
										<input
											type="text"
											class="ui-input ui-input-sm ui-table-filter text-xs"
											placeholder="Filter..."
											value={columnFilters().title || ""}
											onInput={(e) => updateColumnFilter("title", e.currentTarget.value)}
											onClick={(e) => e.stopPropagation()}
										/>
									</div>
								</th>

								<For each={fields()}>
									{(field) => (
										<th scope="col" class="ui-table-header-cell sticky top-0 z-10">
											<div class="flex flex-col gap-2">
												<button
													type="button"
													class="ui-table-header-button select-none"
													onClick={() => handleHeaderClick(field)}
												>
													{field}
													<SortIcon active={sortField() === field} direction={sortDirection()} />
												</button>
												<input
													type="text"
													class="ui-input ui-input-sm ui-table-filter text-xs"
													placeholder="Filter..."
													value={columnFilters()[field] || ""}
													onInput={(e) => updateColumnFilter(field, e.currentTarget.value)}
													onClick={(e) => e.stopPropagation()}
												/>
											</div>
										</th>
									)}
								</For>

								<th scope="col" class="ui-table-header-cell sticky top-0 z-10">
									<div class="flex flex-col gap-2">
										<button
											type="button"
											class="ui-table-header-button select-none"
											onClick={() => handleHeaderClick("updated_at")}
										>
											Updated
											<SortIcon active={sortField() === "updated_at"} direction={sortDirection()} />
										</button>
										<input
											type="text"
											class="ui-input ui-input-sm ui-table-filter text-xs"
											placeholder="Filter..."
											value={columnFilters().updated_at || ""}
											onInput={(e) => updateColumnFilter("updated_at", e.currentTarget.value)}
											onClick={(e) => e.stopPropagation()}
										/>
									</div>
								</th>
							</tr>
						</thead>
						<tbody class="ui-table-body">
							<For each={processedEntries()}>
								{(entry, rowIndex) => (
									<tr class="ui-table-row">
										<td class="ui-table-cell ui-table-cell-muted whitespace-nowrap">
											<button
												type="button"
												onClick={() => props.onEntryClick(entry.id)}
												class="ui-button ui-button-secondary ui-button-sm inline-flex items-center gap-2 text-xs"
												title="View Entry"
												aria-label="View Entry"
											>
												<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
													<title>View Entry</title>
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
													/>
												</svg>
												<span>View</span>
											</button>
										</td>
										{/* biome-ignore lint/a11y/useKeyWithClickEvents: drag select is mouse-only for now */}
										<td
											class={`ui-table-cell whitespace-nowrap font-medium ${
												isSelected(rowIndex(), 0) ? "ui-table-cell-selected" : ""
											}`}
											onMouseDown={() => handleCellMouseDown(rowIndex(), 0)}
											onMouseEnter={(e) => handleCellMouseEnter(e, rowIndex(), 0)}
											onClick={(e) => {
												if (isEditMode()) {
													e.stopPropagation();
													setEditingCell({ id: entry.id, field: "title" });
												}
											}}
										>
											<Show
												when={isCellEditing(entry.id, "title")}
												fallback={entry.title || "Untitled"}
											>
												<input
													value={entry.title || ""}
													onBlur={(e) => {
														const newVal = e.currentTarget.value;
														handleCellUpdate(entry.id, "title", newVal);
														if (
															editingCell()?.id === entry.id &&
															editingCell()?.field === "title"
														) {
															setEditingCell(null);
														}
													}}
													onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
													class="ui-table-cell-input"
													autofocus
													onClick={(e) => e.stopPropagation()}
												/>
											</Show>
										</td>
										<For each={fields()}>
											{(field, fieldIndex) => (
												// biome-ignore lint/a11y/useKeyWithClickEvents: drag select is mouse-only for now
												<td
													class={`ui-table-cell ui-table-cell-muted whitespace-nowrap ${
														isSelected(rowIndex(), fieldIndex() + 1) ? "ui-table-cell-selected" : ""
													}`}
													onMouseDown={() => handleCellMouseDown(rowIndex(), fieldIndex() + 1)}
													onMouseEnter={(e) =>
														handleCellMouseEnter(e, rowIndex(), fieldIndex() + 1)
													}
													onClick={(e) => {
														if (isEditMode()) {
															e.stopPropagation();
															setEditingCell({ id: entry.id, field });
														}
													}}
												>
													<Show
														when={isCellEditing(entry.id, field)}
														fallback={String(entry.properties?.[field] ?? "-")}
													>
														<input
															value={String(entry.properties?.[field] ?? "")}
															onBlur={(e) => {
																const newVal = e.currentTarget.value;
																handleCellUpdate(entry.id, field, newVal);
																if (
																	editingCell()?.id === entry.id &&
																	editingCell()?.field === field
																) {
																	setEditingCell(null);
																}
															}}
															onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
															class="ui-table-cell-input"
															autofocus
															onClick={(e) => e.stopPropagation()}
														/>
													</Show>
												</td>
											)}
										</For>
										<td
											class={`ui-table-cell ui-table-cell-muted whitespace-nowrap ${
												isSelected(rowIndex(), fields().length + 1) ? "ui-table-cell-selected" : ""
											}`}
											onMouseDown={() => handleCellMouseDown(rowIndex(), fields().length + 1)}
											onMouseEnter={(e) => handleCellMouseEnter(e, rowIndex(), fields().length + 1)}
										>
											{new Date(entry.updated_at).toLocaleDateString()}
										</td>
									</tr>
								)}
							</For>
						</tbody>
					</table>
				</div>
			</div>
		</div>
	);
}
