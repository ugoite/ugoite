import { For, createResource, createSignal } from "solid-js";
import type { Schema } from "~/lib/types";
import { workspaceApi } from "~/lib/client";

interface SchemaTableProps {
	workspaceId: string;
	schema: Schema;
	onNoteClick: (noteId: string) => void;
}

export function SchemaTable(props: SchemaTableProps) {
	const [filterText, setFilterText] = createSignal("");

	const [notes] = createResource(
		() => {
			if (!props.workspaceId) return null;
			return { id: props.workspaceId, schema: props.schema.name };
		},
		async ({ id, schema }) => {
			return await workspaceApi.query(id, { class: schema });
		},
	);

	const filteredNotes = () => {
		const allNotes = notes() || [];
		const text = filterText().toLowerCase();
		if (!text) return allNotes;

		return allNotes.filter((note) => {
			// Check title
			if (note.title?.toLowerCase().includes(text)) return true;
			// Check properties
			for (const key in props.schema.fields) {
				const val = String(note.properties[key] || "").toLowerCase();
				if (val.includes(text)) return true;
			}
			return false;
		});
	};

	return (
		<div class="flex-1 h-full overflow-auto bg-white dark:bg-gray-950">
			<div class="p-6">
				<div class="mb-6">
					<h1 class="text-2xl font-bold text-gray-900 dark:text-white mb-2">{props.schema.name}</h1>
					<p class="text-gray-500 dark:text-gray-400 text-sm mb-4">
						{notes.loading ? "Loading..." : `${filteredNotes().length} records found`}
					</p>

					<input
						type="text"
						placeholder="Filter records..."
						class="w-full max-w-md px-4 py-2 border border-gray-300 dark:border-gray-700 rounded-lg bg-white dark:bg-gray-900 text-gray-900 dark:text-white focus:ring-2 focus:ring-blue-500 focus:border-transparent outline-none transition-shadow"
						value={filterText()}
						onInput={(e) => setFilterText(e.currentTarget.value)}
					/>
				</div>

				<div class="overflow-x-auto border border-gray-200 dark:border-gray-800 rounded-lg shadow-sm">
					<table class="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
						<thead class="bg-gray-50 dark:bg-gray-900">
							<tr>
								<th
									scope="col"
									class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider"
								>
									Title
								</th>
								<For each={Object.keys(props.schema.fields)}>
									{(field) => (
										<th
											scope="col"
											class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider"
										>
											{field}
										</th>
									)}
								</For>
								<th
									scope="col"
									class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider"
								>
									Updated
								</th>
							</tr>
						</thead>
						<tbody class="bg-white dark:bg-gray-950 divide-y divide-gray-200 dark:divide-gray-800">
							<For each={filteredNotes()}>
								{(note) => (
									<tr
										class="hover:bg-gray-50 dark:hover:bg-gray-900 cursor-pointer transition-colors"
										onClick={() => props.onNoteClick(note.id)}
									>
										<td class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900 dark:text-white">
											{note.title || "Untitled"}
										</td>
										<For each={Object.keys(props.schema.fields)}>
											{(field) => (
												<td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400">
													{String(note.properties[field] || "-")}
												</td>
											)}
										</For>
										<td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400">
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
