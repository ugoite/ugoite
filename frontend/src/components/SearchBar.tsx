import { createSignal, Show, onMount, onCleanup, createEffect } from "solid-js";

export interface SearchBarProps {
	onSearch: (query: string) => void;
	loading?: boolean;
	resultsCount?: number;
	placeholder?: string;
}

/**
 * SearchBar component for searching entries in space.
 * Supports keyboard shortcut (Cmd/Ctrl+K) to focus search.
 * Triggers real-time search as user types with debouncing to avoid blocking input.
 */
export function SearchBar(props: SearchBarProps) {
	const [query, setQuery] = createSignal("");
	let inputRef: HTMLInputElement | undefined;

	// Trigger search in real-time as user types, with debouncing to avoid blocking
	createEffect(() => {
		const searchQuery = query().trim();

		// Use setTimeout to debounce and avoid blocking input
		const timeoutId = setTimeout(() => {
			// Run search in next event loop to never block input
			setTimeout(() => {
				props.onSearch(searchQuery);
			}, 0);
		}, 150); // 150ms debounce

		// Cleanup timeout on next effect run
		onCleanup(() => clearTimeout(timeoutId));
	});

	const handleSubmit = (e: Event) => {
		e.preventDefault();
		// Search is already triggered by createEffect, but we can keep this for explicit enter key
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
			<search class="relative" onSubmit={handleSubmit}>
				<div class="relative flex items-center">
					{/* Search Icon */}
					<svg
						class="absolute left-3 w-5 h-5 ui-muted"
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
						placeholder={props.placeholder || "Search entries... (⌘K)"}
						class="ui-input w-full pl-10 pr-20"
					/>

					{/* Clear Button */}
					<Show when={query()}>
						<button
							type="button"
							onClick={handleClear}
							aria-label="Clear search"
							class="ui-button ui-button-secondary ui-button-sm absolute right-2"
						>
							<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
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
			</search>

			{/* Status Messages */}
			<div class="mt-2 text-sm">
				<Show when={props.loading}>
					<span class="ui-muted">Searching...</span>
				</Show>
				<Show when={props.resultsCount !== undefined && !props.loading}>
					<span class="ui-muted">
						{props.resultsCount} {props.resultsCount === 1 ? "result" : "results"}
					</span>
				</Show>
			</div>
		</div>
	);
}
