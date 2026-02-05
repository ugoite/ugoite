// REQ-FE-004: Entry list display
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { CanvasPlaceholder } from "./CanvasPlaceholder";
import type { EntryRecord } from "~/lib/types";

describe("CanvasPlaceholder", () => {
	const mockEntries: EntryRecord[] = [
		{
			id: "entry-1",
			title: "First Entry",
			updated_at: "2025-01-01T00:00:00Z",
			properties: { Status: "Active", Priority: "High" },
			tags: [],
			links: [],
		},
		{
			id: "entry-2",
			title: "Second Entry",
			updated_at: "2025-01-02T00:00:00Z",
			properties: { Category: "Work" },
			tags: [],
			links: [{ id: "link-1", target: "entry-1", kind: "related" }],
		},
	];

	it("should render canvas area with placeholder message", () => {
		render(() => <CanvasPlaceholder entries={[]} />);

		expect(screen.getByTestId("canvas-placeholder")).toBeInTheDocument();
		expect(screen.getByText(/milestone 6/i)).toBeInTheDocument();
	});

	it("should display entries as cards", () => {
		render(() => <CanvasPlaceholder entries={mockEntries} />);

		expect(screen.getByText("First Entry")).toBeInTheDocument();
		expect(screen.getByText("Second Entry")).toBeInTheDocument();
	});

	it("should show entry properties on cards", () => {
		render(() => <CanvasPlaceholder entries={mockEntries} />);

		expect(screen.getByText("Status:")).toBeInTheDocument();
		expect(screen.getByText("Active")).toBeInTheDocument();
	});

	it("should show links indicator when entry has links", () => {
		render(() => <CanvasPlaceholder entries={mockEntries} />);

		expect(screen.getByText("1 links")).toBeInTheDocument();
	});

	it("should call onSelect when card is clicked", () => {
		const onSelect = vi.fn();
		render(() => <CanvasPlaceholder entries={mockEntries} onSelect={onSelect} />);

		fireEvent.click(screen.getByText("First Entry"));

		expect(onSelect).toHaveBeenCalledWith("entry-1");
	});

	it("should highlight selected entry", () => {
		render(() => <CanvasPlaceholder entries={mockEntries} selectedEntryId="entry-1" />);

		const cards = screen.getAllByTestId("canvas-entry-card");
		expect(cards[0]).toHaveClass("border-blue-500");
		expect(cards[1]).not.toHaveClass("border-blue-500");
	});

	it("should show empty state when no entries", () => {
		render(() => <CanvasPlaceholder entries={[]} />);

		expect(screen.getByText(/no entries to display/i)).toBeInTheDocument();
	});

	it("should use canvas_position if provided", () => {
		const entryWithPosition: EntryRecord = {
			id: "positioned-entry",
			title: "Positioned Entry",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
			canvas_position: { x: 500, y: 300 },
		};

		render(() => <CanvasPlaceholder entries={[entryWithPosition]} />);

		const card = screen.getByTestId("canvas-entry-card");
		expect(card.style.left).toBe("500px");
		// +60 for banner offset
		expect(card.style.top).toBe("360px");
	});
});
