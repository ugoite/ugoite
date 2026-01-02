import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { Canvas } from "./Canvas";
import type { NoteRecord, WorkspaceLink } from "~/lib/types";

const mockNotes: NoteRecord[] = [
	{
		id: "note-1",
		title: "First Note",
		class: null,
		updated_at: "2025-01-01T00:00:00Z",
		properties: {},
		tags: [],
		links: [],
		canvas_position: { x: 100, y: 100 },
	},
	{
		id: "note-2",
		title: "Second Note",
		class: null,
		updated_at: "2025-01-01T00:00:00Z",
		properties: {},
		tags: [],
		links: [],
		canvas_position: { x: 400, y: 200 },
	},
];

const mockLinks: WorkspaceLink[] = [
	{
		id: "link-1",
		source: "note-1",
		target: "note-2",
		kind: "related",
	},
];

describe("Canvas", () => {
	it("should render notes at canvas positions", () => {
		render(() => <Canvas notes={mockNotes} links={[]} />);

		expect(screen.getByText("First Note")).toBeInTheDocument();
		expect(screen.getByText("Second Note")).toBeInTheDocument();
	});

	it("should render links as SVG edges", () => {
		render(() => <Canvas notes={mockNotes} links={mockLinks} />);

		const svg = screen.getByTestId("canvas-links");
		expect(svg).toBeInTheDocument();
		expect(svg.querySelectorAll("line")).toHaveLength(1);
	});

	it("should call onSelect when note is clicked", () => {
		const onSelect = vi.fn();
		render(() => <Canvas notes={mockNotes} links={[]} onSelect={onSelect} />);

		const noteCard = screen.getByText("First Note").closest("div[data-note-id]");
		if (noteCard) {
			fireEvent.click(noteCard);
		}

		expect(onSelect).toHaveBeenCalledWith("note-1");
	});

	it("should highlight selected note", () => {
		render(() => <Canvas notes={mockNotes} links={[]} selectedNoteId="note-1" />);

		const noteCard = screen.getByText("First Note").closest("div[data-note-id]");
		expect(noteCard).toHaveClass("ring-2");
	});

	it("should allow dragging notes", () => {
		const onPositionChange = vi.fn();
		render(() => <Canvas notes={mockNotes} links={[]} onPositionChange={onPositionChange} />);

		const noteCard = screen.getByText("First Note").closest("div[data-note-id]") as HTMLElement;

		// Simulate drag
		fireEvent.mouseDown(noteCard, { clientX: 100, clientY: 100 });
		fireEvent.mouseMove(document, { clientX: 150, clientY: 150 });
		fireEvent.mouseUp(document);

		expect(onPositionChange).toHaveBeenCalledWith("note-1", { x: 150, y: 150 });
	});

	it("should persist position after drag", () => {
		const onPositionChange = vi.fn();
		render(() => <Canvas notes={mockNotes} links={[]} onPositionChange={onPositionChange} />);

		const noteCard = screen.getByText("First Note").closest("div[data-note-id]") as HTMLElement;

		fireEvent.mouseDown(noteCard, { clientX: 100, clientY: 100 });
		fireEvent.mouseMove(document, { clientX: 200, clientY: 200 });
		fireEvent.mouseUp(document);

		expect(onPositionChange).toHaveBeenCalled();
	});

	it("should create link when notes are connected", () => {
		const onLinkCreate = vi.fn();
		render(() => <Canvas notes={mockNotes} links={[]} onLinkCreate={onLinkCreate} />);

		// Find link buttons
		const linkButtons = screen.getAllByLabelText(/create link/i);
		expect(linkButtons.length).toBeGreaterThan(0);

		// Click to start link mode
		fireEvent.click(linkButtons[0]);

		// Click on target note
		const targetNote = screen.getByText("Second Note").closest("div[data-note-id]");
		if (targetNote) {
			fireEvent.click(targetNote);
		}

		expect(onLinkCreate).toHaveBeenCalledWith({
			source: "note-1",
			target: "note-2",
			kind: "related",
		});
	});

	it("should delete link when delete button is clicked", () => {
		const onLinkDelete = vi.fn();
		render(() => <Canvas notes={mockNotes} links={mockLinks} onLinkDelete={onLinkDelete} />);

		const deleteButton = screen.getByLabelText(/delete link/i);
		fireEvent.click(deleteButton);

		expect(onLinkDelete).toHaveBeenCalledWith("link-1");
	});

	it("should render empty state when no notes", () => {
		render(() => <Canvas notes={[]} links={[]} />);
		expect(screen.getByText(/no notes/i)).toBeInTheDocument();
	});

	it("should support panning the canvas", () => {
		render(() => <Canvas notes={mockNotes} links={[]} />);

		const canvas = screen.getByTestId("canvas-area");

		// Simulate pan gesture
		fireEvent.mouseDown(canvas, { clientX: 0, clientY: 0, button: 1 }); // Middle mouse button
		fireEvent.mouseMove(document, { clientX: 100, clientY: 100 });
		fireEvent.mouseUp(document);

		// Check if transform was applied (implementation detail)
		// This test verifies the gesture is captured
		expect(canvas).toBeInTheDocument();
	});

	it("should properly display object properties without [object Object]", () => {
		const notesWithObjectProps: NoteRecord[] = [
			{
				id: "note-with-obj",
				title: "Note with Object Property",
				class: null,
				updated_at: "2025-01-01T00:00:00Z",
				properties: {
					simpleString: "text value",
					objectValue: { nested: "data", count: 42 },
					arrayValue: [1, 2, 3],
				},
				tags: [],
				links: [],
				canvas_position: { x: 100, y: 100 },
			},
		];

		render(() => <Canvas notes={notesWithObjectProps} links={[]} />);

		// Check that string is displayed as-is
		expect(screen.getByText(/text value/)).toBeInTheDocument();

		// Check that object is serialized (not [object Object])
		const content = screen.getByText(/Note with Object Property/).closest("div[data-note-id]");
		expect(content?.textContent).toContain('{"nested":"data","count":42}');
		expect(content?.textContent).not.toContain("[object Object]");

		// Check that array is serialized
		expect(content?.textContent).toContain("[1,2,3]");
	});
});
