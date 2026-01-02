import { createSignal, Show, onMount, onCleanup } from "solid-js";

export interface SearchBarProps {
	onSearch: (query: string) => void;
	loading?: boolean;
	resultsCount?: number;
	placeholder?: string;
}

/**
 * SearchBar component for searching notes in workspace.
 * Supports keyboard shortcut (Cmd/Ctrl+K) to focus search.
 */
export function SearchBar(props: SearchBarProps) {
	const [query, setQuery] = createSignal("");
	let inputRef: HTMLInputElement | undefined;

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		const searchQuery = query().trim();
		if (searchQuery) {
			props.onSearch(searchQuery);
		}
	};

	const handleClear = () => {
		setQuery("");
		inputRef?.focus();
	};

	// Keyboard shortcut handler
	const handleKeyDown = (e: KeyboardEvent) => {
		// Cmd/Ctrl + K to focus search
		if ((e.metaKey || e.ctrlKey) && e.key === "k") {
			e.preventDefault();
			inputRef?.focus();
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

	return (
		<div class="search-bar">
			<form role="search" onSubmit={handleSubmit} class="relative">
				<div class="relative flex items-center">
					{/* Search Icon */}
					<svg
						class="absolute left-3 w-5 h-5 text-gray-400"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
					>
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
						/>
					</svg>

					{/* Input Field */}
					<input
						ref={inputRef}
						type="text"
						value={query()}
						onInput={(e) => setQuery(e.currentTarget.value)}
						placeholder={props.placeholder || "Search notes... (⌘K)"}
						class="w-full pl-10 pr-20 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
						disabled={props.loading}
					/>

					{/* Clear Button */}
					<Show when={query()}>
						<button
							type="button"
							onClick={handleClear}
							aria-label="Clear search"
							class="absolute right-2 p-1 hover:bg-gray-100 rounded"
						>
							<svg
								class="w-4 h-4 text-gray-500"
								fill="none"
								stroke="currentColor"
								viewBox="0 0 24 24"
							>
								<path
									stroke-linecap="round"
									stroke-linejoin="round"
									stroke-width="2"
									d="M6 18L18 6M6 6l12 12"
								/>
							</svg>
						</button>
					</Show>
				</div>
			</form>

			{/* Status Messages */}
			<div class="mt-2 text-sm">
				<Show when={props.loading}>
					<span class="text-gray-500">Searching...</span>
				</Show>
				<Show when={props.resultsCount !== undefined && !props.loading}>
					<span class="text-gray-600">
						{props.resultsCount} {props.resultsCount === 1 ? "result" : "results"}
					</span>
				</Show>
			</div>
		</div>
	);
}
