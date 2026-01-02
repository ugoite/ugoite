import { createSignal, For, Show, createEffect } from "solid-js";
import type { NoteRecord, WorkspaceLink } from "~/lib/types";

export interface CanvasProps {
	notes: NoteRecord[];
	links: WorkspaceLink[];
	selectedNoteId?: string | null;
	onSelect?: (noteId: string) => void;
	onLinkCreate?: (link: { source: string; target: string; kind: string }) => void;
	onLinkDelete?: (linkId: string) => void;
}

interface NodePosition {
	x: number;
	y: number;
}

interface LinkingState {
	sourceNoteId: string | null;
}

/**
 * Simple Graph view for visualizing note connections.
 * Uses automatic layout instead of drag-and-drop for cross-device compatibility.
 * Implements Story 4: "The Connection Map"
 */
export function Canvas(props: CanvasProps) {
	const [nodePositions, setNodePositions] = createSignal<Map<string, NodePosition>>(new Map());
	const [linkingState, setLinkingState] = createSignal<LinkingState>({ sourceNoteId: null });

	// Simple auto-layout: circular arrangement
	createEffect(() => {
		const notes = props.notes;
		if (notes.length === 0) return;

		const positions = new Map<string, NodePosition>();
		const centerX = 400;
		const centerY = 300;
		const radius = Math.min(300, 100 + notes.length * 20);

		notes.forEach((note, index) => {
			const angle = (index / notes.length) * 2 * Math.PI - Math.PI / 2;
			positions.set(note.id, {
				x: centerX + radius * Math.cos(angle),
				y: centerY + radius * Math.sin(angle),
			});
		});

		setNodePositions(positions);
	});

	// Get node position with fallback
	const getNodePosition = (noteId: string): NodePosition => {
		return nodePositions().get(noteId) || { x: 400, y: 300 };
	};

	// Note click handler
	const handleNoteClick = (noteId: string) => {
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
	const handleCancelLinking = () => {
		setLinkingState({ sourceNoteId: null });
	};

	return (
		<div
			role="application"
			class="canvas relative w-full h-full bg-gray-50 overflow-hidden"
			data-testid="canvas-area"
		>
			{/* SVG Layer for links */}
			<svg class="absolute inset-0" style={{ "z-index": "0" }} data-testid="canvas-links">
				<For each={props.links}>
					{(link) => {
						const sourcePos = getNodePosition(link.source);
						const targetPos = getNodePosition(link.target);

						return (
							<g>
								<line
									x1={sourcePos.x}
									y1={sourcePos.y}
									x2={targetPos.x}
									y2={targetPos.y}
									stroke="#3b82f6"
									stroke-width="2"
									opacity="0.6"
								/>
								{/* Link delete button */}
								<foreignObject
									x={(sourcePos.x + targetPos.x) / 2 - 12}
									y={(sourcePos.y + targetPos.y) / 2 - 12}
									width="24"
									height="24"
									class="pointer-events-auto"
								>
									<button
										type="button"
										onClick={() => props.onLinkDelete?.(link.id)}
										aria-label="Delete link"
										class="w-6 h-6 bg-red-500 text-white rounded-full hover:bg-red-600 flex items-center justify-center text-sm font-bold shadow-md"
									>
										×
									</button>
								</foreignObject>
							</g>
						);
					}}
				</For>
			</svg>

			{/* Linking Mode Banner */}
			<Show when={linkingState().sourceNoteId}>
				<div class="absolute top-4 left-1/2 transform -translate-x-1/2 bg-blue-500 text-white px-4 py-2 rounded-lg shadow-lg z-50 flex items-center gap-3">
					<span>Click another note to create a link</span>
					<button type="button" onClick={handleCancelLinking} class="underline hover:no-underline">
						Cancel
					</button>
				</div>
			</Show>

			{/* Empty State */}
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
									d="M9 20l-5.447-2.724A1 1 0 013 16.382V5.618a1 1 0 011.447-.894L9 7m0 13l6-3m-6 3V7m6 10l4.553 2.276A1 1 0 0021 18.382V7.618a1 1 0 00-.553-.894L15 4m0 13V4m0 0L9 7"
								/>
							</svg>
							<p class="text-lg">No notes to display</p>
							<p class="text-sm mt-2">Create some notes and links to see the graph</p>
						</div>
					</div>
				}
			>
				{/* Nodes Layer */}
				<For each={props.notes}>
					{(note) => {
						const pos = () => getNodePosition(note.id);
						const isSelected = () => props.selectedNoteId === note.id;
						const isLinking = () => linkingState().sourceNoteId === note.id;

						return (
							<div
								role="button"
								tabIndex={0}
								data-note-id={note.id}
								class="absolute bg-white rounded-lg shadow-md hover:shadow-xl transition-all cursor-pointer transform -translate-x-1/2 -translate-y-1/2"
								classList={{
									"ring-4 ring-blue-500 scale-105": isSelected(),
									"ring-4 ring-green-500": isLinking(),
								}}
								style={{
									left: `${pos().x}px`,
									top: `${pos().y}px`,
									width: "220px",
									"z-index": isSelected() ? "20" : "10",
								}}
								onClick={() => handleNoteClick(note.id)}
								onKeyDown={(e) => {
									if (e.key === "Enter" || e.key === " ") {
										e.preventDefault();
										handleNoteClick(note.id);
									}
								}}
							>
								{/* Card Content */}
								<div class="p-3">
									<div class="flex items-start justify-between mb-2">
										<h3 class="font-semibold text-gray-900 text-sm flex-1 line-clamp-2">
											{note.title || "Untitled"}
										</h3>
										<button
											type="button"
											onClick={(e) => handleStartLinking(e, note.id)}
											aria-label="Create link"
											class="ml-2 p-1 hover:bg-gray-100 rounded flex-shrink-0"
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

									{/* Tags */}
									<Show when={note.tags.length > 0}>
										<div class="flex flex-wrap gap-1 mb-2">
											<For each={note.tags.slice(0, 2)}>
												{(tag) => (
													<span class="px-2 py-0.5 bg-blue-100 text-blue-700 rounded text-xs">
														{tag}
													</span>
												)}
											</For>
											<Show when={note.tags.length > 2}>
												<span class="text-xs text-gray-500">+{note.tags.length - 2}</span>
											</Show>
										</div>
									</Show>

									{/* Link count */}
									<Show when={note.links.length > 0}>
										<div class="flex items-center gap-1 text-xs text-gray-500">
											<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
												<path
													stroke-linecap="round"
													stroke-linejoin="round"
													stroke-width="2"
													d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1"
												/>
											</svg>
											<span>{note.links.length}</span>
										</div>
									</Show>
								</div>
							</div>
						);
					}}
				</For>
			</Show>
		</div>
	);
}
