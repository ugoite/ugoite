import { For, Show } from "solid-js";
import { SearchBar } from "./SearchBar";
import { WorkspaceSelector, type WorkspaceSelectorProps } from "./WorkspaceSelector";
import type { Schema } from "~/lib/types";

export interface SidebarProps extends WorkspaceSelectorProps {
	/** Current view mode */
	viewMode: "notes" | "schemas";
	/** Callback when view mode changes */
	onViewModeChange: (mode: "notes" | "schemas") => void;
	/** Available schemas for filtering/creating */
	schemas: Schema[];
	/** Selected class filter */
	selectedClass: string;
	/** Callback when class filter changes */
	onClassFilterChange: (className: string) => void;
	/** Loading state for search */
	searchLoading?: boolean;
	/** Results count for search */
	searchResultsCount?: number;
	/** Callback when search query changes */
	onSearch: (query: string) => void;
	/** Callback when create button is clicked */
	onCreate: () => void;
	/** Create button label */
	createButtonLabel: string;
	/** Children to render in the list area */
	children: JSX.Element;
}

/**
 * Unified sidebar component for Notes and Data Models views.
 * Provides workspace selection, view mode switching, search, filtering, and list display.
 */
export function Sidebar(props: SidebarProps) {
	return (
		<aside class="w-80 flex-shrink-0 bg-white border-r flex flex-col h-full">
			<WorkspaceSelector
				workspaces={props.workspaces}
				selectedWorkspaceId={props.selectedWorkspaceId}
				loading={props.loading}
				error={props.error}
				onSelect={props.onSelect}
				onCreate={props.onCreate}
			/>

			{/* View Mode Switcher */}
			<div class="flex border-b border-gray-200 dark:border-gray-800">
				<button
					type="button"
					onClick={() => props.onViewModeChange("notes")}
					class={`flex-1 py-3 text-sm font-medium text-center ${
						props.viewMode === "notes"
							? "text-blue-600 border-b-2 border-blue-600 bg-blue-50 dark:bg-blue-900/20"
							: "text-gray-500 hover:text-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
					}`}
				>
					Notes
				</button>
				<button
					type="button"
					onClick={() => props.onViewModeChange("schemas")}
					class={`flex-1 py-3 text-sm font-medium text-center ${
						props.viewMode === "schemas"
							? "text-blue-600 border-b-2 border-blue-600 bg-blue-50 dark:bg-blue-900/20"
							: "text-gray-500 hover:text-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800"
					}`}
				>
					Data Models
				</button>
			</div>

			{/* Create Button */}
			<div class="p-4 border-b">
				<button
					type="button"
					onClick={props.onCreate}
					class="w-full px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 flex items-center justify-center gap-2"
				>
					<svg
						class="w-5 h-5"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
						aria-hidden="true"
					>
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M12 4v16m8-8H4"
						/>
					</svg>
					{props.createButtonLabel}
				</button>
			</div>

			{/* Search */}
			<div class="p-4 border-b">
				<SearchBar
					onSearch={props.onSearch}
					loading={props.searchLoading}
					resultsCount={props.searchResultsCount}
					placeholder={
						props.viewMode === "notes" ? "Search notes... (⌘K)" : "Search data models... (⌘K)"
					}
				/>
			</div>

			{/* Class Filter */}
			<Show when={props.schemas.length > 0}>
				<div class="px-4 pb-4 border-b">
					<label class="block text-xs font-medium text-gray-600 mb-1" for="class-filter">
						Filter by Class
					</label>
					<select
						id="class-filter"
						class="w-full px-3 py-2 border border-gray-300 rounded-lg bg-white text-gray-900 text-sm"
						value={props.selectedClass}
						onChange={(e) => props.onClassFilterChange(e.currentTarget.value)}
					>
						<option value="">All</option>
						<For each={props.schemas}>{(s) => <option value={s.name}>{s.name}</option>}</For>
					</select>
				</div>
			</Show>

			{/* List Content */}
			<div class="flex-1 overflow-auto p-4">{props.children}</div>
		</aside>
	);
}
