import { For, Show, createMemo } from "solid-js";
import type { Accessor } from "solid-js";
import type { EntryRecord, Form } from "~/lib/types";
import { SearchBar } from "./SearchBar";

export interface ListPanelProps {
	/** Current view mode */
	mode: "entries" | "forms";
	/** Whether to show the create button */
	showCreate?: boolean;
	/** Label for create button */
	createLabel?: string;
	/** Whether the create button is disabled */
	createDisabled?: boolean;
	/** Optional helper text when create is disabled */
	createDisabledReason?: string;
	/** Callback when create button is clicked */
	onCreate?: () => void;
	/** List of forms for filter dropdown */
	forms: Form[];
	/** Selected form for filtering */
	filterForm: Accessor<string>;
	/** Callback when filter form changes */
	onFilterFormChange: (formName: string) => void;
	/** Search callback */
	onSearch?: (query: string) => void;
	/** Whether search is in progress */
	isSearching?: boolean;
	/** Number of search results */
	searchResultsCount?: number;
	/** Entries to display (for entries mode) */
	entries?: EntryRecord[];
	/** Loading state */
	loading?: boolean;
	/** Error state */
	error?: string | null;
	/** Selected item ID */
	selectedId?: string;
	/** Callback when a entry is selected */
	onSelectEntry?: (entryId: string) => void;
	/** Callback when a form is selected */
	onSelectForm?: (entryForm: Form) => void;
	/** Currently selected form (for forms mode) */
	selectedForm?: Form | null;
}

export function ListPanel(props: ListPanelProps) {
	const entries = createMemo(() => props.entries || []);
	const loading = createMemo(() => props.loading || false);
	const error = createMemo(() => props.error || null);

	return (
		<div class="flex flex-col h-full overflow-hidden">
			{/* Create button */}
			<Show when={props.showCreate !== false}>
				<div class="p-4 ui-divider">
					<button
						type="button"
						onClick={props.onCreate}
						disabled={props.createDisabled}
						title={props.createDisabled ? props.createDisabledReason : undefined}
						class="ui-button ui-button-primary w-full flex items-center justify-center gap-2"
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
						{props.createLabel || (props.mode === "entries" ? "New Entry" : "New Form")}
					</button>
					<Show when={props.createDisabled && props.createDisabledReason}>
						<p class="mt-2 text-xs ui-muted">{props.createDisabledReason}</p>
					</Show>
				</div>
			</Show>

			{/* Search (for entries mode) */}
			<Show when={props.mode === "entries" && props.onSearch}>
				{(onSearch) => (
					<div class="p-4 ui-divider space-y-3">
						<SearchBar
							onSearch={onSearch()}
							loading={props.isSearching || false}
							resultsCount={props.searchResultsCount || 0}
						/>
					</div>
				)}
			</Show>

			{/* Form filter */}
			<Show when={props.mode === "entries" && props.forms.length > 0}>
				<div class="px-4 py-3 ui-divider">
					<label class="ui-label text-xs mb-1" for="filter-form">
						Filter by Form
					</label>
					<select
						id="filter-form"
						class="ui-input w-full text-sm"
						value={props.filterForm()}
						onChange={(e) => props.onFilterFormChange(e.currentTarget.value)}
					>
						<option value="">All</option>
						<For each={props.forms}>{(s) => <option value={s.name}>{s.name}</option>}</For>
					</select>
				</div>
			</Show>

			{/* List content */}
			<div class="flex-1 overflow-auto p-4">
				<Show when={props.mode === "entries"}>
					<EntryListContent
						entries={entries}
						loading={loading}
						error={error}
						selectedId={props.selectedId}
						onSelect={props.onSelectEntry}
					/>
				</Show>

				<Show when={props.mode === "forms"}>
					<FormListContent
						forms={props.forms}
						selectedForm={props.selectedForm || null}
						onSelect={props.onSelectForm}
					/>
				</Show>
			</div>
		</div>
	);
}

interface EntryListContentProps {
	entries: Accessor<EntryRecord[]>;
	loading: Accessor<boolean>;
	error: Accessor<string | null>;
	selectedId?: string;
	onSelect?: (entryId: string) => void;
}

function EntryListContent(props: EntryListContentProps) {
	return (
		<div class="entry-list-container">
			<Show when={props.loading()}>
				<div class="loading-indicator flex items-center justify-center p-8">
					<div class="flex flex-col items-center">
						<svg
							class="animate-spin h-8 w-8 ui-accent-text mb-2"
							xmlns="http://www.w3.org/2000/svg"
							fill="none"
							viewBox="0 0 24 24"
						>
							<circle
								class="opacity-25"
								cx="12"
								cy="12"
								r="10"
								stroke="currentColor"
								stroke-width="4"
							/>
							<path
								class="opacity-75"
								fill="currentColor"
								d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
							/>
						</svg>
						<span class="ui-muted text-sm">Loading entries...</span>
					</div>
				</div>
			</Show>

			<Show when={props.error()}>
				<div class="error-message ui-alert ui-alert-error flex items-start gap-2">
					<svg class="w-5 h-5 flex-shrink-0 mt-0.5" fill="currentColor" viewBox="0 0 20 20">
						<path
							fill-rule="evenodd"
							d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z"
							clip-rule="evenodd"
						/>
					</svg>
					<span>{props.error()}</span>
				</div>
			</Show>

			<Show when={!props.loading() && props.entries().length === 0 && !props.error()}>
				<div class="empty-state p-12 text-center">
					<div class="flex flex-col items-center">
						<svg
							class="w-16 h-16 ui-muted mb-4"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="1.5"
								d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
							/>
						</svg>
						<p class="ui-muted font-medium mb-1">No entries yet</p>
						<Show
							when={props.mode === "entries" && props.forms.length === 0}
							fallback={<p class="text-sm ui-muted">Create your first entry to get started</p>}
						>
							<p class="text-sm ui-muted">Create a form first to start writing entries</p>
						</Show>
					</div>
				</div>
			</Show>

			<Show when={!props.loading() && props.entries().length > 0}>
				<ul class="space-y-2">
					<For each={props.entries()}>
						{(entry) => (
							<EntryListItem
								entry={entry}
								isSelected={props.selectedId === entry.id}
								onClick={() => props.onSelect?.(entry.id)}
							/>
						)}
					</For>
				</ul>
			</Show>
		</div>
	);
}

interface EntryListItemProps {
	entry: EntryRecord;
	isSelected: boolean;
	onClick: () => void;
}

function EntryListItem(props: EntryListItemProps) {
	const propertyEntries = () => Object.entries(props.entry.properties ?? {}).slice(0, 3);

	const formatDate = (dateStr: string) => {
		try {
			return new Date(dateStr).toLocaleDateString();
		} catch {
			return dateStr;
		}
	};

	return (
		<li data-testid="entry-item">
			<button
				type="button"
				class={`entry-item-button ui-card ui-card-interactive ui-card-hover w-full text-left cursor-pointer transition-all duration-200 ${
					props.isSelected ? "ui-card-selected" : ""
				}`}
				onClick={props.onClick}
				aria-pressed={props.isSelected}
			>
				<div class="flex justify-between items-start mb-2">
					<h3 class="font-semibold truncate flex-1 pr-2">{props.entry.title || "Untitled"}</h3>
					<Show when={props.entry.form}>
						<span class="ui-pill text-xs whitespace-nowrap">{props.entry.form}</span>
					</Show>
				</div>

				<Show when={propertyEntries().length > 0}>
					<div class="mt-2 text-sm ui-muted space-y-1">
						<For each={propertyEntries()}>
							{([key, value]) => (
								<div class="flex items-baseline">
									<span class="ui-label mr-2 text-xs uppercase tracking-wide">{key}:</span>
									<span class="truncate">
										{typeof value === "string" ? value : JSON.stringify(value)}
									</span>
								</div>
							)}
						</For>
					</div>
				</Show>

				<div class="mt-3 text-xs ui-muted flex items-center">
					<svg
						class="w-3 h-3 mr-1 opacity-50"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
					>
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
						/>
					</svg>
					<span>Updated {formatDate(props.entry.updated_at)}</span>
				</div>
			</button>
		</li>
	);
}

interface FormListContentProps {
	forms: Form[];
	selectedForm: Form | null;
	onSelect?: (entryForm: Form) => void;
}

function FormListContent(props: FormListContentProps) {
	return (
		<Show
			when={props.forms.length > 0}
			fallback={
				<div class="empty-state p-12 text-center">
					<div class="flex flex-col items-center">
						<svg
							class="w-16 h-16 ui-muted mb-4"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="1.5"
								d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 5c0 2.21-3.582 4-8 4s-8-1.79-8-4"
							/>
						</svg>
						<p class="ui-muted font-medium mb-1">No forms yet</p>
						<p class="text-sm ui-muted">Create your first form to get started</p>
					</div>
				</div>
			}
		>
			<ul class="space-y-2">
				<For each={props.forms}>
					{(entryForm) => (
						<li>
							<button
								type="button"
								onClick={() => props.onSelect?.(entryForm)}
								class={`ui-card ui-card-interactive ui-card-hover w-full text-left cursor-pointer transition-all duration-200 ${
									props.selectedForm?.name === entryForm.name ? "ui-card-selected" : ""
								}`}
							>
								<div class="font-semibold">{entryForm.name}</div>
								<div class="text-xs ui-muted mt-1">
									{Object.keys(entryForm.fields).length}{" "}
									{Object.keys(entryForm.fields).length === 1 ? "field" : "fields"}
								</div>
							</button>
						</li>
					)}
				</For>
			</ul>
		</Show>
	);
}
