import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { CanvasPlaceholder } from "./CanvasPlaceholder";
import type { NoteRecord } from "~/lib/types";

describe("CanvasPlaceholder", () => {
	const mockNotes: NoteRecord[] = [
		{
			id: "note-1",
			title: "First Note",
			updated_at: "2025-01-01T00:00:00Z",
			properties: { Status: "Active", Priority: "High" },
			tags: [],
			links: [],
		},
		{
			id: "note-2",
			title: "Second Note",
			updated_at: "2025-01-02T00:00:00Z",
			properties: { Category: "Work" },
			tags: [],
			links: [{ id: "link-1", target: "note-1", kind: "related" }],
		},
	];

	it("should render canvas area with placeholder message", () => {
		render(() => <CanvasPlaceholder notes={[]} />);

		expect(screen.getByTestId("canvas-placeholder")).toBeInTheDocument();
		expect(screen.getByText(/milestone 6/i)).toBeInTheDocument();
	});

	it("should display notes as cards", () => {
		render(() => <CanvasPlaceholder notes={mockNotes} />);

		expect(screen.getByText("First Note")).toBeInTheDocument();
		expect(screen.getByText("Second Note")).toBeInTheDocument();
	});

	it("should show note properties on cards", () => {
		render(() => <CanvasPlaceholder notes={mockNotes} />);

		expect(screen.getByText("Status:")).toBeInTheDocument();
		expect(screen.getByText("Active")).toBeInTheDocument();
	});

	it("should show links indicator when note has links", () => {
		render(() => <CanvasPlaceholder notes={mockNotes} />);

		expect(screen.getByText("1 links")).toBeInTheDocument();
	});

	it("should call onSelect when card is clicked", () => {
		const onSelect = vi.fn();
		render(() => <CanvasPlaceholder notes={mockNotes} onSelect={onSelect} />);

		fireEvent.click(screen.getByText("First Note"));

		expect(onSelect).toHaveBeenCalledWith("note-1");
	});

	it("should highlight selected note", () => {
		render(() => (
			<CanvasPlaceholder notes={mockNotes} selectedNoteId="note-1" />
		));

		const cards = screen.getAllByTestId("canvas-note-card");
		expect(cards[0]).toHaveClass("border-blue-500");
		expect(cards[1]).not.toHaveClass("border-blue-500");
	});

	it("should show empty state when no notes", () => {
		render(() => <CanvasPlaceholder notes={[]} />);

		expect(screen.getByText(/no notes to display/i)).toBeInTheDocument();
	});

	it("should use canvas_position if provided", () => {
		const noteWithPosition: NoteRecord = {
			id: "positioned-note",
			title: "Positioned Note",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
			canvas_position: { x: 500, y: 300 },
		};

		render(() => <CanvasPlaceholder notes={[noteWithPosition]} />);

		const card = screen.getByTestId("canvas-note-card");
		expect(card.style.left).toBe("500px");
		// +60 for banner offset
		expect(card.style.top).toBe("360px");
	});
});
