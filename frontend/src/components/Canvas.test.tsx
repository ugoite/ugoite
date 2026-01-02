import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { Canvas } from "./Canvas";
import type { NoteRecord, WorkspaceLink } from "~/lib/types";

const mockNotes: NoteRecord[] = [
	{
		id: "note-1",
		title: "First Note",
		class: null,
		updated_at: "2025-01-01T00:00:00Z",
		properties: {},
		tags: ["tag1"],
		links: [],
	},
	{
		id: "note-2",
		title: "Second Note",
		class: null,
		updated_at: "2025-01-01T00:00:00Z",
		properties: {},
		tags: [],
		links: [],
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
	it("should render notes with auto-layout", async () => {
		render(() => <Canvas notes={mockNotes} links={[]} />);

		await waitFor(() => {
			expect(screen.getByText("First Note")).toBeInTheDocument();
			expect(screen.getByText("Second Note")).toBeInTheDocument();

			// Check that notes have positioning (auto-layout applied with class)
			const note1 = screen.getByText("First Note").closest("div[data-note-id]") as HTMLElement;
			expect(note1).toHaveClass("absolute");
			expect(note1?.style.left).toBeTruthy();
			expect(note1?.style.top).toBeTruthy();
		});
	});

	it("should render links as visual connections", async () => {
		render(() => <Canvas notes={mockNotes} links={mockLinks} />);

		const svg = screen.getByTestId("canvas-links");
		expect(svg).toBeInTheDocument();

		await waitFor(() => {
			expect(svg.querySelectorAll("line")).toHaveLength(1);
		});
	});

	it("should select note on click", async () => {
		const onSelect = vi.fn();
		render(() => <Canvas notes={mockNotes} links={[]} onSelect={onSelect} />);

		await waitFor(() => {
			const noteCard = screen.getByText("First Note").closest("div[data-note-id]");
			expect(noteCard).toBeInTheDocument();
		});

		const noteCard = screen.getByText("First Note").closest("div[data-note-id]");
		if (noteCard) {
			fireEvent.click(noteCard);
		}

		expect(onSelect).toHaveBeenCalledWith("note-1");
	});

	it("should highlight selected note", async () => {
		render(() => <Canvas notes={mockNotes} links={[]} selectedNoteId="note-1" />);

		await waitFor(() => {
			const noteCard = screen.getByText("First Note").closest("div[data-note-id]");
			expect(noteCard).toHaveClass("ring-4");
		});
	});

	it("should create link between notes", async () => {
		const onLinkCreate = vi.fn();
		render(() => <Canvas notes={mockNotes} links={[]} onLinkCreate={onLinkCreate} />);

		await waitFor(() => {
			expect(screen.getByText("First Note")).toBeInTheDocument();
		});

		// Find link button and start linking
		const linkButtons = screen.getAllByLabelText(/create link/i);
		fireEvent.click(linkButtons[0]);

		// Verify linking mode banner appears
		await waitFor(() => {
			expect(screen.getByText(/click another note to create a link/i)).toBeInTheDocument();
		});

		// Click target note to complete link
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

	it("should delete link", async () => {
		const onLinkDelete = vi.fn();
		render(() => <Canvas notes={mockNotes} links={mockLinks} onLinkDelete={onLinkDelete} />);

		await waitFor(() => {
			expect(screen.getByLabelText(/delete link/i)).toBeInTheDocument();
		});

		const deleteButton = screen.getByLabelText(/delete link/i);
		fireEvent.click(deleteButton);

		expect(onLinkDelete).toHaveBeenCalledWith("link-1");
	});

	it("should render empty state when no notes", () => {
		render(() => <Canvas notes={[]} links={[]} />);
		expect(screen.getByText(/no notes to display/i)).toBeInTheDocument();
	});

	it("should properly display tags", async () => {
		render(() => <Canvas notes={mockNotes} links={[]} />);

		await waitFor(() => {
			expect(screen.getByText("tag1")).toBeInTheDocument();
		});
	});

	it("should display link count for notes with links", async () => {
		const notesWithLinks: NoteRecord[] = [
			{
				...mockNotes[0],
				links: [
					{ id: "l1", source: "note-1", target: "note-2", kind: "related" },
					{ id: "l2", source: "note-1", target: "note-3", kind: "related" },
				],
			},
		];

		render(() => <Canvas notes={notesWithLinks} links={[]} />);

		await waitFor(() => {
			expect(screen.getByText("2")).toBeInTheDocument();
		});
	});

	it("should cancel linking mode when cancel button clicked", async () => {
		render(() => <Canvas notes={mockNotes} links={[]} />);

		await waitFor(() => {
			expect(screen.getByText("First Note")).toBeInTheDocument();
		});

		// Start linking
		const linkButtons = screen.getAllByLabelText(/create link/i);
		fireEvent.click(linkButtons[0]);

		await waitFor(() => {
			expect(screen.getByText(/click another note to create a link/i)).toBeInTheDocument();
		});

		// Cancel linking
		const cancelButton = screen.getByText(/cancel/i);
		fireEvent.click(cancelButton);

		await waitFor(() => {
			expect(screen.queryByText(/click another note to create a link/i)).not.toBeInTheDocument();
		});
	});
});
