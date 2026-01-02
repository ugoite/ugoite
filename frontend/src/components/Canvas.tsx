import { createSignal, For, Show, onMount, onCleanup } from "solid-js";
import type { NoteRecord, WorkspaceLink } from "~/lib/types";

export interface CanvasProps {
	notes: NoteRecord[];
	links: WorkspaceLink[];
	selectedNoteId?: string | null;
	onSelect?: (noteId: string) => void;
	onPositionChange?: (noteId: string, position: { x: number; y: number }) => void;
	onLinkCreate?: (link: { source: string; target: string; kind: string }) => void;
	onLinkDelete?: (linkId: string) => void;
}

interface DragState {
	noteId: string;
	startX: number;
	startY: number;
	offsetX: number;
	offsetY: number;
}

interface LinkingState {
	sourceNoteId: string | null;
}

/**
 * Interactive Canvas component for organizing notes spatially with drag-drop and linking.
 * Implements Story 4: "The Infinite Canvas"
 */
export function Canvas(props: CanvasProps) {
	const [dragState, setDragState] = createSignal<DragState | null>(null);
	const [linkingState, setLinkingState] = createSignal<LinkingState>({ sourceNoteId: null });
	const [panOffset, setPanOffset] = createSignal({ x: 0, y: 0 });
	const [isPanning, setIsPanning] = createSignal(false);
	const [panStart, setPanStart] = createSignal({ x: 0, y: 0 });

	// Get note position (use canvas_position or generate default)
	const getNotePosition = (note: NoteRecord, index: number) => {
		if (note.canvas_position) {
			return note.canvas_position;
		}
		// Default grid layout
		const col = index % 3;
		const row = Math.floor(index / 3);
		return {
			x: 100 + col * 350,
			y: 100 + row * 250,
		};
	};

	// Mouse down on note card - start dragging
	const handleNoteMouseDown = (
		e: MouseEvent,
		noteId: string,
		position: { x: number; y: number },
	) => {
		if (e.button !== 0) return; // Only left click

		e.stopPropagation();
		e.preventDefault();

		setDragState({
			noteId,
			startX: e.clientX,
			startY: e.clientY,
			offsetX: position.x,
			offsetY: position.y,
		});
	};

	// Mouse move - update drag position
	const handleMouseMove = (e: MouseEvent) => {
		const drag = dragState();
		if (drag) {
			const dx = e.clientX - drag.startX;
			const dy = e.clientY - drag.startY;

			// Update position dynamically during drag
			const newPos = {
				x: drag.offsetX + dx,
				y: drag.offsetY + dy,
			};

			// Update the note's position in the DOM
			const noteEl = document.querySelector(`[data-note-id="${drag.noteId}"]`) as HTMLElement;
			if (noteEl) {
				noteEl.style.left = `${newPos.x}px`;
				noteEl.style.top = `${newPos.y}px`;
			}
		}

		// Handle panning
		if (isPanning()) {
			const start = panStart();
			setPanOffset({
				x: e.clientX - start.x,
				y: e.clientY - start.y,
			});
		}
	};

	// Mouse up - finalize drag
	const handleMouseUp = () => {
		const drag = dragState();
		if (drag && props.onPositionChange) {
			const noteEl = document.querySelector(`[data-note-id="${drag.noteId}"]`) as HTMLElement;
			if (noteEl) {
				const finalX = Number.parseInt(noteEl.style.left, 10);
				const finalY = Number.parseInt(noteEl.style.top, 10);
				props.onPositionChange(drag.noteId, { x: finalX, y: finalY });
			}
		}
		setDragState(null);
		setIsPanning(false);
	};

	// Note click handler
	const handleNoteClick = (_e: MouseEvent, noteId: string) => {
		if (dragState()) return; // Ignore clicks during drag

		const linking = linkingState();
		if (linking.sourceNoteId) {
			// Complete link creation
			if (linking.sourceNoteId !== noteId && props.onLinkCreate) {
				props.onLinkCreate({
					source: linking.sourceNoteId,
					target: noteId,
					kind: "related",
				});
			}
			setLinkingState({ sourceNoteId: null });
		} else if (props.onSelect) {
			props.onSelect(noteId);
		}
	};

	// Start linking mode
	const handleStartLinking = (e: MouseEvent, noteId: string) => {
		e.stopPropagation();
		setLinkingState({ sourceNoteId: noteId });
	};

	// Cancel linking
	const handleCancelLinking = (e: MouseEvent) => {
		e.stopPropagation();
		setLinkingState({ sourceNoteId: null });
	};

	// Pan canvas (middle mouse button)
	const handleCanvasMouseDown = (e: MouseEvent) => {
		if (e.button === 1) {
			// Middle mouse button
			e.preventDefault();
			setIsPanning(true);
			setPanStart({ x: e.clientX, y: e.clientY });
		}
	};

	// Setup global mouse event listeners
	onMount(() => {
		document.addEventListener("mousemove", handleMouseMove);
		document.addEventListener("mouseup", handleMouseUp);
	});

	onCleanup(() => {
		document.removeEventListener("mousemove", handleMouseMove);
		document.removeEventListener("mouseup", handleMouseUp);
	});

	// Render SVG lines for links
	const renderLinks = () => {
		return (
			<svg
				class="absolute inset-0 pointer-events-none"
				style={{ "z-index": "0" }}
				data-testid="canvas-links"
			>
				<For each={props.links}>
					{(link) => {
						const sourceNote = props.notes.find((n) => n.id === link.source);
						const targetNote = props.notes.find((n) => n.id === link.target);

						if (!sourceNote || !targetNote) return null;

						const sourcePos = getNotePosition(sourceNote, props.notes.indexOf(sourceNote));
						const targetPos = getNotePosition(targetNote, props.notes.indexOf(targetNote));

						// Center of note cards (approximate)
						const x1 = sourcePos.x + 140;
						const y1 = sourcePos.y + 100;
						const x2 = targetPos.x + 140;
						const y2 = targetPos.y + 100;

						return (
							<g>
								<line
									x1={x1}
									y1={y1}
									x2={x2}
									y2={y2}
									stroke="#3b82f6"
									stroke-width="2"
									marker-end="url(#arrowhead)"
								/>
								{/* Link delete button */}
								<foreignObject
									x={(x1 + x2) / 2 - 10}
									y={(y1 + y2) / 2 - 10}
									width="20"
									height="20"
									class="pointer-events-auto"
								>
									<button
										type="button"
										onClick={() => props.onLinkDelete?.(link.id)}
										aria-label="Delete link"
										class="w-5 h-5 bg-red-500 text-white rounded-full hover:bg-red-600 flex items-center justify-center text-xs"
									>
										×
									</button>
								</foreignObject>
							</g>
						);
					}}
				</For>
				{/* Arrow marker definition */}
				<defs>
					<marker id="arrowhead" markerWidth="10" markerHeight="10" refX="9" refY="3" orient="auto">
						<polygon points="0 0, 10 3, 0 6" fill="#3b82f6" />
					</marker>
				</defs>
			</svg>
		);
	};

	return (
		<div
			role="application"
			class="canvas relative w-full h-full bg-gray-50 overflow-hidden"
			onMouseDown={handleCanvasMouseDown}
			data-testid="canvas-area"
			style={{ cursor: isPanning() ? "grabbing" : "default" }}
		>
			{/* Grid Background */}
			<div
				class="absolute inset-0 pointer-events-none"
				style={{
					"background-image": "radial-gradient(circle, #ddd 1px, transparent 1px)",
					"background-size": "20px 20px",
					transform: `translate(${panOffset().x}px, ${panOffset().y}px)`,
				}}
			/>

			{/* Links Layer */}
			{renderLinks()}

			{/* Linking Mode Banner */}
			<Show when={linkingState().sourceNoteId}>
				<div class="absolute top-4 left-1/2 transform -translate-x-1/2 bg-blue-500 text-white px-4 py-2 rounded-lg shadow-lg z-50">
					<span>Click another note to create a link</span>
					<button
						type="button"
						onClick={handleCancelLinking}
						class="ml-4 underline hover:no-underline"
					>
						Cancel
					</button>
				</div>
			</Show>

			{/* Notes Layer */}
			<div class="absolute inset-0" style={{ "z-index": "1" }}>
				<Show
					when={props.notes.length > 0}
					fallback={
						<div class="flex items-center justify-center h-full">
							<div class="text-center text-gray-400">
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
								<p>No notes to display</p>
							</div>
						</div>
					}
				>
					<For each={props.notes}>
						{(note, index) => {
							const pos = () => getNotePosition(note, index());
							const isSelected = () => props.selectedNoteId === note.id;
							const isLinking = () => linkingState().sourceNoteId === note.id;

							return (
								<div
									role="button"
									tabIndex={0}
									data-note-id={note.id}
									class="absolute bg-white rounded-lg shadow-md hover:shadow-lg transition-shadow cursor-move"
									classList={{
										"ring-2 ring-blue-500": isSelected(),
										"ring-2 ring-green-500": isLinking(),
									}}
									style={{
										left: `${pos().x}px`,
										top: `${pos().y}px`,
										width: "280px",
										"min-height": "160px",
									}}
									onMouseDown={(e) => handleNoteMouseDown(e, note.id, pos())}
									onClick={(e) => handleNoteClick(e, note.id)}
									onKeyDown={(e) => {
										if (e.key === "Enter" || e.key === " ") {
											e.preventDefault();
											handleNoteClick(e as unknown as MouseEvent, note.id);
										}
									}}
								>
									{/* Card Header */}
									<div class="p-4 border-b flex items-center justify-between">
										<h3 class="font-semibold text-gray-900 truncate flex-1">
											{note.title || "Untitled"}
										</h3>
										<button
											type="button"
											onClick={(e) => handleStartLinking(e, note.id)}
											aria-label="Create link"
											class="ml-2 p-1 hover:bg-gray-100 rounded"
										>
											<svg
												class="w-4 h-4 text-gray-600"
												fill="none"
												stroke="currentColor"
												viewBox="0 0 24 24"
											>
												<path
													stroke-linecap="round"
													stroke-linejoin="round"
													stroke-width="2"
													d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1"
												/>
											</svg>
										</button>
									</div>

									{/* Card Body */}
									<div class="p-4">
										{/* Properties */}
										<Show when={Object.keys(note.properties).length > 0}>
											<div class="mb-2 space-y-1">
												<For each={Object.entries(note.properties).slice(0, 3)}>
													{([key, value]) => {
														// Properly serialize property values
														const displayValue =
															typeof value === "object" && value !== null
																? JSON.stringify(value)
																: String(value);
														return (
															<div class="text-xs">
																<span class="font-medium text-gray-600">{key}:</span>
																<span class="text-gray-800 ml-1">{displayValue}</span>
															</div>
														);
													}}
												</For>
											</div>
										</Show>{" "}
										{/* Tags */}
										<Show when={note.tags.length > 0}>
											<div class="flex flex-wrap gap-1 mb-2">
												<For each={note.tags.slice(0, 3)}>
													{(tag) => (
														<span class="px-2 py-1 bg-blue-100 text-blue-700 rounded text-xs">
															{tag}
														</span>
													)}
												</For>
											</div>
										</Show>
										{/* Links indicator */}
										<Show when={note.links.length > 0}>
											<div class="flex items-center gap-1 text-xs text-gray-500 mt-2">
												<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1"
													/>
												</svg>
												<span>
													{note.links.length} {note.links.length === 1 ? "link" : "links"}
												</span>
											</div>
										</Show>
									</div>
								</div>
							);
						}}
					</For>
				</Show>
			</div>
		</div>
	);
}
