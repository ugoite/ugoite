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
import type { Schema, NoteRecord } from "~/lib/types";
import { workspaceApi, noteApi } from "~/lib/client";
import { replaceFirstH1, ensureClassFrontmatter, updateH2Section } from "~/lib/markdown";

interface SchemaTableProps {
	workspaceId: string;
	schema: Schema;
	onNoteClick: (noteId: string) => void;
}

type SortDirection = "asc" | "desc" | null;

/** Helper to filter notes by title or properties */
function filterNotes(notes: NoteRecord[], fields: string[], query: string) {
	if (!query) return notes;
	const text = query.toLowerCase();
	return notes.filter((note) => {
		const title = (note.title || "").toLowerCase();
		if (title.includes(text)) return true;
		for (const field of fields) {
			const val = String(note.properties?.[field] ?? "").toLowerCase();
			if (val.includes(text)) return true;
		}
		return false;
	});
}

/** Helper for column-specific filtering */
function applyColumnFilters(notes: NoteRecord[], filters: Record<string, string>) {
	const activeFilters = Object.entries(filters).filter(([_, val]) => !!val);
	if (activeFilters.length === 0) return notes;

	return notes.filter((note) => {
		for (const [field, filter] of activeFilters) {
			const filterLower = filter.toLowerCase();
			let val = "";
			if (field === "title") val = note.title || "";
			else if (field === "updated_at") val = new Date(note.updated_at).toLocaleDateString();
			else val = String(note.properties?.[field] ?? "");

			if (!val.toLowerCase().includes(filterLower)) return false;
		}
		return true;
	});
}

function sortNotes(notes: NoteRecord[], field: string, direction: SortDirection) {
	if (!field || !direction) return notes;
	return [...notes].sort((a, b) => {
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
				class="w-4 h-4 text-gray-400 opacity-50"
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
			<svg class="w-4 h-4 text-blue-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
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
		<svg class="w-4 h-4 text-blue-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
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

/** Helper to format a single note as a CSV row */
function formatCsvRow(note: NoteRecord, headers: string[]) {
	return headers
		.map((field) => {
			let val = "";
			if (field === "title") val = note.title || "";
			else if (field === "updated_at") {
				try {
					val = new Date(note.updated_at).toISOString();
				} catch {
					val = note.updated_at;
				}
			} else {
				val = String(note.properties?.[field] ?? "");
			}
			return `"${val.replace(/"/g, '""')}"`;
		})
		.join(",");
}

export function SchemaTable(props: SchemaTableProps) {
	// State for filtering and sorting
	const [globalFilter, setGlobalFilter] = createSignal("");
	const [sortField, setSortField] = createSignal<string | null>(null);
	const [sortDirection, setSortDirection] = createSignal<SortDirection>(null);
	const [columnFilters, setColumnFilters] = createSignal<Record<string, string>>({});
	const [isEditMode, setIsEditMode] = createSignal(false);
	const [editingCell, setEditingCell] = createSignal<{ id: string; field: string } | null>(null);

	const [notes, { refetch, mutate }] = createResource(
		() => {
			if (!props.workspaceId || !props.schema?.name) return false;
			return { id: props.workspaceId, schemaName: props.schema.name };
		},
		async ({ id, schemaName }) => {
			return await workspaceApi.query(id, { class: schemaName });
		},
	);

	const fields = createMemo(() => (props.schema?.fields ? Object.keys(props.schema.fields) : []));

	const processedNotes = createMemo(() => {
		const currentNotes = notes();
		if (!currentNotes) return [] as NoteRecord[];

		// 1. Global Filter
		let result = filterNotes([...currentNotes], fields(), globalFilter());

		// 2. Column Filters
		result = applyColumnFilters(result, columnFilters());

		// 3. Sorting
		return sortNotes(result, sortField() || "", sortDirection());
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
			const { data, fieldNames, schemaName } = untrack(() => ({
				data: processedNotes(),
				fieldNames: fields(),
				schemaName: props.schema?.name || "export",
			}));

			const headers = ["title", ...fieldNames, "updated_at"];
			const csvContent = [headers.join(","), ...data.map((n) => formatCsvRow(n, headers))].join(
				"\n",
			);

			const blob = new Blob([csvContent], { type: "text/csv;charset=utf-8;" });
			const url = URL.createObjectURL(blob);
			const link = document.createElement("a");
			link.setAttribute("href", url);
			link.setAttribute("download", `${schemaName}_export.csv`);
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
			let content = props.schema.template || `# New ${props.schema.name}\n`;
			content = ensureClassFrontmatter(content, props.schema.name);

			await noteApi.create(props.workspaceId, {
				content,
			});
			refetch();
		} catch (err) {
			// biome-ignore lint/suspicious/noConsole: error logging
			console.error("Failed to add row", err);
			alert(`Failed to add row: ${err instanceof Error ? err.message : String(err)}`);
		}
	};

	const handleCellUpdate = async (noteId: string, field: string, value: string) => {
		try {
			// Fetch full note to get content and revision_id
			const note = await noteApi.get(props.workspaceId, noteId);
			let updatedMarkdown = note.content;

			if (field === "title") {
				updatedMarkdown = replaceFirstH1(updatedMarkdown, value);
			} else {
				updatedMarkdown = updateH2Section(updatedMarkdown, field, value);
			}

			await noteApi.update(props.workspaceId, noteId, {
				markdown: updatedMarkdown,
				parent_revision_id: note.revision_id,
			});

			// Partial rendering: update local state without full refetch
			mutate((prev) => {
				if (!prev) return prev;
				return prev.map((n) => {
					if (n.id === noteId) {
						const next = { ...n };
						if (field === "title") {
							next.title = value;
						} else {
							next.properties = { ...n.properties, [field]: value };
						}
						next.updated_at = new Date().toISOString();
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

	const getRowData = (note: NoteRecord, currentFields: string[], c1: number, c2: number) => {
		const rowData = [];
		// Col 0: Title
		if (c1 <= 0 && c2 >= 0) rowData.push(note.title || "");

		// Cols 1..N: Fields
		for (let i = 0; i < currentFields.length; i++) {
			const colIdx = i + 1;
			if (colIdx >= c1 && colIdx <= c2) {
				rowData.push(String(note.properties?.[currentFields[i]] ?? ""));
			}
		}

		// Col N+1: Updated
		const lastCol = currentFields.length + 1;
		if (c1 <= lastCol && c2 >= lastCol) {
			rowData.push(new Date(note.updated_at).toLocaleDateString());
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

		const currentNotes = processedNotes();
		const currentFields = fields();

		const rowsData = [];
		for (let r = r1; r <= r2; r++) {
			const note = currentNotes[r];
			if (!note) continue;
			rowsData.push(getRowData(note, currentFields, c1, c2));
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
			e.preventDefault();
			await copySelection();
		}
	};

	const handleCellMouseDown = (r: number, c: number) => {
		setIsSelecting(true);
		setSelection({ start: { r, c }, end: { r, c } });
	};

	const handleCellMouseEnter = (r: number, c: number) => {
		if (isSelecting()) {
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
		<div
			class={`flex-1 h-full overflow-auto bg-white dark:bg-gray-950 ${
				isSelecting() ? "select-none" : ""
			}`}
		>
			<div class="p-6">
				<div class="mb-6 flex justify-between items-start">
					<div>
						<h1 class="text-2xl font-bold text-gray-900 dark:text-white mb-2">
							{props.schema.name}
						</h1>
						<p class="text-gray-500 dark:text-gray-400 text-sm">
							{notes.loading && !notes()
								? "Loading..."
								: `${processedNotes().length} records found`}
						</p>
					</div>
					<div class="flex gap-2">
						<button
							type="button"
							onClick={downloadCSV}
							class="px-4 py-2 bg-green-600 hover:bg-green-700 text-white rounded-lg flex items-center gap-2 text-sm font-medium transition-colors"
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
							class={`px-4 py-2 rounded-lg flex items-center gap-2 text-sm font-medium transition-colors ${
								isEditMode()
									? "bg-blue-100 text-blue-700 hover:bg-blue-200 dark:bg-blue-900 dark:text-blue-100"
									: "bg-gray-100 text-gray-700 hover:bg-gray-200 dark:bg-gray-800 dark:text-gray-200"
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
								class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg flex items-center gap-2 text-sm font-medium transition-colors"
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
						class="w-full max-w-md px-4 py-2 border border-gray-300 dark:border-gray-700 rounded-lg bg-white dark:bg-gray-900 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 focus:border-transparent outline-none transition-shadow"
						value={globalFilter()}
						onInput={(e) => setGlobalFilter(e.currentTarget.value)}
					/>
				</div>

				<div class="overflow-x-auto border border-gray-200 dark:border-gray-800 rounded-lg shadow-sm">
					<table class="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
						<thead class="bg-gray-50 dark:bg-gray-900">
							<tr>
								<th
									scope="col"
									class="w-10 px-4 py-3 text-left bg-gray-50 dark:bg-gray-900 sticky top-0 z-10"
								>
									{/* Action Column for Link */}
								</th>
								<th
									scope="col"
									class="px-6 py-3 text-left bg-gray-50 dark:bg-gray-900 sticky top-0 z-10"
								>
									<div class="flex flex-col gap-2">
										<button
											type="button"
											class="flex items-center gap-1 cursor-pointer select-none text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider group hover:text-gray-700 dark:hover:text-gray-300"
											onClick={() => handleHeaderClick("title")}
										>
											Title
											<SortIcon active={sortField() === "title"} direction={sortDirection()} />
										</button>
										<input
											type="text"
											class="w-full px-2 py-1 text-xs border rounded bg-white dark:bg-gray-800 border-gray-300 dark:border-gray-700"
											placeholder="Filter..."
											value={columnFilters().title || ""}
											onInput={(e) => updateColumnFilter("title", e.currentTarget.value)}
											onClick={(e) => e.stopPropagation()}
										/>
									</div>
								</th>

								<For each={fields()}>
									{(field) => (
										<th
											scope="col"
											class="px-6 py-3 text-left bg-gray-50 dark:bg-gray-900 sticky top-0 z-10"
										>
											<div class="flex flex-col gap-2">
												<button
													type="button"
													class="flex items-center gap-1 cursor-pointer select-none text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider group hover:text-gray-700 dark:hover:text-gray-300"
													onClick={() => handleHeaderClick(field)}
												>
													{field}
													<SortIcon active={sortField() === field} direction={sortDirection()} />
												</button>
												<input
													type="text"
													class="w-full px-2 py-1 text-xs border rounded bg-white dark:bg-gray-800 border-gray-300 dark:border-gray-700"
													placeholder="Filter..."
													value={columnFilters()[field] || ""}
													onInput={(e) => updateColumnFilter(field, e.currentTarget.value)}
													onClick={(e) => e.stopPropagation()}
												/>
											</div>
										</th>
									)}
								</For>

								<th
									scope="col"
									class="px-6 py-3 text-left bg-gray-50 dark:bg-gray-900 sticky top-0 z-10"
								>
									<div class="flex flex-col gap-2">
										<button
											type="button"
											class="flex items-center gap-1 cursor-pointer select-none text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider group hover:text-gray-700 dark:hover:text-gray-300"
											onClick={() => handleHeaderClick("updated_at")}
										>
											Updated
											<SortIcon active={sortField() === "updated_at"} direction={sortDirection()} />
										</button>
										<input
											type="text"
											class="w-full px-2 py-1 text-xs border rounded bg-white dark:bg-gray-800 border-gray-300 dark:border-gray-700"
											placeholder="Filter..."
											value={columnFilters().updated_at || ""}
											onInput={(e) => updateColumnFilter("updated_at", e.currentTarget.value)}
											onClick={(e) => e.stopPropagation()}
										/>
									</div>
								</th>
							</tr>
						</thead>
						<tbody class="bg-white dark:bg-gray-950 divide-y divide-gray-200 dark:divide-gray-800">
							<For each={processedNotes()}>
								{(note, rowIndex) => (
									<tr class="hover:bg-gray-50 dark:hover:bg-gray-900 transition-colors">
										<td class="px-4 py-4 whitespace-nowrap text-sm font-medium text-gray-500">
											<button
												type="button"
												onClick={() => props.onNoteClick(note.id)}
												class="text-blue-600 hover:text-blue-900 dark:text-blue-400 dark:hover:text-blue-300 transition-colors"
												title="View Note"
												aria-label="View Note"
											>
												<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
													<title>View Note</title>
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
													/>
												</svg>
											</button>
										</td>
										{/* biome-ignore lint/a11y/useKeyWithClickEvents: drag select is mouse-only for now */}
										<td
											class={`px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900 dark:text-white ${
												isSelected(rowIndex(), 0) ? "bg-blue-100 dark:bg-blue-900" : ""
											}`}
											onMouseDown={() => handleCellMouseDown(rowIndex(), 0)}
											onMouseEnter={() => handleCellMouseEnter(rowIndex(), 0)}
											onClick={(e) => {
												if (isEditMode()) {
													e.stopPropagation();
													setEditingCell({ id: note.id, field: "title" });
												}
											}}
										>
											<Show
												when={isCellEditing(note.id, "title")}
												fallback={note.title || "Untitled"}
											>
												<input
													value={note.title || ""}
													onBlur={(e) => {
														handleCellUpdate(note.id, "title", e.currentTarget.value);
														setEditingCell(null);
													}}
													onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
													class="w-full px-2 py-1 border rounded bg-white dark:bg-gray-800 dark:text-white"
													autofocus
													onClick={(e) => e.stopPropagation()}
												/>
											</Show>
										</td>
										<For each={fields()}>
											{(field, fieldIndex) => (
												// biome-ignore lint/a11y/useKeyWithClickEvents: drag select is mouse-only for now
												<td
													class={`px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400 ${
														isSelected(rowIndex(), fieldIndex() + 1)
															? "bg-blue-100 dark:bg-blue-900"
															: ""
													}`}
													onMouseDown={() => handleCellMouseDown(rowIndex(), fieldIndex() + 1)}
													onMouseEnter={() => handleCellMouseEnter(rowIndex(), fieldIndex() + 1)}
													onClick={(e) => {
														if (isEditMode()) {
															e.stopPropagation();
															setEditingCell({ id: note.id, field });
														}
													}}
												>
													<Show
														when={isCellEditing(note.id, field)}
														fallback={String(note.properties?.[field] ?? "-")}
													>
														<input
															value={String(note.properties?.[field] ?? "")}
															onBlur={(e) => {
																handleCellUpdate(note.id, field, e.currentTarget.value);
																setEditingCell(null);
															}}
															onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
															class="w-full px-2 py-1 border rounded bg-white dark:bg-gray-800 dark:text-white"
															autofocus
															onClick={(e) => e.stopPropagation()}
														/>
													</Show>
												</td>
											)}
										</For>
										<td
											class={`px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400 ${
												isSelected(rowIndex(), fields().length + 1)
													? "bg-blue-100 dark:bg-blue-900"
													: ""
											}`}
											onMouseDown={() => handleCellMouseDown(rowIndex(), fields().length + 1)}
											onMouseEnter={() => handleCellMouseEnter(rowIndex(), fields().length + 1)}
										>
											{new Date(note.updated_at).toLocaleDateString()}
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
