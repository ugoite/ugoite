import { For, Show } from "solid-js";
import type { EntryRecord } from "~/lib/types";

export interface CanvasPlaceholderProps {
	entries: EntryRecord[];
	selectedEntryId?: string | null;
	onSelect?: (entryId: string) => void;
}

/**
 * Canvas placeholder component for Milestone 5.
 * Displays entries in a static grid layout as a preview of future infinite canvas functionality.
 * Story 4 (The Infinite Canvas) will be fully implemented in Milestone 6.
 */
export function CanvasPlaceholder(props: CanvasPlaceholderProps) {
	// Calculate static positions based on index
	const getPosition = (index: number) => {
		const cols = 3;
		const cardWidth = 280;
		const cardHeight = 200;
		const gap = 20;
		const offsetX = 40;
		const offsetY = 40;

		const col = index % cols;
		const row = Math.floor(index / cols);

		return {
			x: offsetX + col * (cardWidth + gap),
			y: offsetY + row * (cardHeight + gap),
		};
	};

	return (
		<div
			class="canvas-placeholder relative w-full h-full ui-surface overflow-auto"
			data-testid="canvas-placeholder"
		>
			{/* Info Banner */}
			<div class="sticky top-0 left-0 right-0 ui-alert ui-alert-warning p-3 z-10">
				<div class="flex items-center gap-2 ui-accent-text">
					<svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
						/>
					</svg>
					<span class="text-sm">
						Canvas View Preview — Full drag-and-drop functionality coming in Milestone 6
					</span>
				</div>
			</div>

			{/* Canvas Area */}
			<div
				class="canvas-area relative min-h-full p-10"
				style={{ "min-width": "1200px", "min-height": "800px" }}
			>
				{/* Grid Background */}
				<div class="absolute inset-0 pointer-events-none ui-canvas-grid" />

				{/* Entry Cards */}
				<Show
					when={props.entries.length > 0}
					fallback={
						<div class="absolute inset-0 flex items-center justify-center">
							<div class="text-center ui-muted">
								<svg
									class="w-16 h-16 mx-auto mb-4 opacity-50"
									fill="none"
									stroke="currentColor"
									viewBox="0 0 24 24"
								>
									<path
										stroke-linecap="round"
										stroke-linejoin="round"
										stroke-width="1"
										d="M4 5a1 1 0 011-1h14a1 1 0 011 1v2a1 1 0 01-1 1H5a1 1 0 01-1-1V5zM4 13a1 1 0 011-1h6a1 1 0 011 1v6a1 1 0 01-1 1H5a1 1 0 01-1-1v-6zM16 13a1 1 0 011-1h2a1 1 0 011 1v6a1 1 0 01-1 1h-2a1 1 0 01-1-1v-6z"
									/>
								</svg>
								<p>No entries to display</p>
								<p class="text-sm mt-1">Create a entry to see it here</p>
							</div>
						</div>
					}
				>
					<For each={props.entries}>
						{(entry, index) => {
							const pos = () => entry.canvas_position || getPosition(index());
							const isSelected = () => props.selectedEntryId === entry.id;

							return (
								<button
									data-testid="canvas-entry-card"
									type="button"
									class={`absolute w-[280px] ui-card ui-card-interactive ui-card-hover transition-all cursor-pointer ${
										isSelected() ? "ui-card-selected" : ""
									}`}
									style={{
										left: `${pos().x}px`,
										top: `${pos().y + 60}px`, // Account for banner
									}}
									onClick={() => props.onSelect?.(entry.id)}
									aria-pressed={isSelected()}
								>
									{/* Card Header */}
									<div class="p-3 ui-card-header">
										<h3 class="font-medium truncate">{entry.title || "Untitled"}</h3>
										<Show when={entry.form}>
											<span class="text-xs ui-muted">{entry.form}</span>
										</Show>
									</div>

									{/* Card Body */}
									<div class="p-3">
										{/* Properties preview */}
										<Show
											when={Object.keys(entry.properties).length > 0}
											fallback={<p class="text-sm ui-muted italic">No properties</p>}
										>
											<div class="space-y-1 text-sm">
												<For each={Object.entries(entry.properties).slice(0, 3)}>
													{([key, value]) => (
														<div class="flex">
															<span class="ui-label font-medium mr-1">{key}:</span>
															<span class="truncate">
																{typeof value === "string" ? value : JSON.stringify(value)}
															</span>
														</div>
													)}
												</For>
												<Show when={Object.keys(entry.properties).length > 3}>
													<span class="text-xs ui-muted">
														+{Object.keys(entry.properties).length - 3} more
													</span>
												</Show>
											</div>
										</Show>

										{/* Links indicator */}
										<Show when={entry.links.length > 0}>
											<div class="mt-2 flex items-center gap-1 text-xs ui-accent-text">
												<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1"
													/>
												</svg>
												<span>{entry.links.length} links</span>
											</div>
										</Show>
									</div>
								</button>
							);
						}}
					</For>
				</Show>
			</div>
		</div>
	);
}
