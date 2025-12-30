import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { NoteList } from "./NoteList";
import { resetMockData, seedWorkspace, seedNote } from "~/test/mocks/handlers";
import type { Note, NoteRecord, Workspace } from "~/lib/types";

const testWorkspace: Workspace = {
	id: "ui-test-ws",
	name: "UI Test Workspace",
	created_at: "2025-01-01T00:00:00Z",
};

describe("NoteList", () => {
	beforeEach(() => {
		resetMockData();
		seedWorkspace(testWorkspace);
	});

	it("should render empty state when no notes exist", async () => {
		render(() => <NoteList workspaceId="ui-test-ws" />);

		await waitFor(() => {
			expect(screen.getByText(/no notes/i)).toBeInTheDocument();
		});
	});

	it("should render list of notes with titles", async () => {
		const note1: Note = {
			id: "note-1",
			content: "# First Note",
			revision_id: "rev-1",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record1: NoteRecord = {
			id: "note-1",
			title: "First Note",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};
		const note2: Note = {
			id: "note-2",
			content: "# Second Note",
			revision_id: "rev-2",
			created_at: "2025-01-02T00:00:00Z",
			updated_at: "2025-01-02T00:00:00Z",
		};
		const record2: NoteRecord = {
			id: "note-2",
			title: "Second Note",
			updated_at: "2025-01-02T00:00:00Z",
			properties: { Status: "Active" },
			tags: [],
			links: [],
		};

		seedNote("ui-test-ws", note1, record1);
		seedNote("ui-test-ws", note2, record2);

		render(() => <NoteList workspaceId="ui-test-ws" />);

		await waitFor(() => {
			expect(screen.getByText("First Note")).toBeInTheDocument();
			expect(screen.getByText("Second Note")).toBeInTheDocument();
		});
	});

	it("should display extracted properties in note cards", async () => {
		const note: Note = {
			id: "prop-note",
			content: "# Meeting\n\n## Date\n2025-01-15\n\n## Status\nCompleted",
			revision_id: "rev-prop",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: NoteRecord = {
			id: "prop-note",
			title: "Meeting",
			updated_at: "2025-01-01T00:00:00Z",
			properties: { Date: "2025-01-15", Status: "Completed" },
			tags: [],
			links: [],
		};

		seedNote("ui-test-ws", note, record);

		render(() => <NoteList workspaceId="ui-test-ws" />);

		await waitFor(() => {
			expect(screen.getByText("Meeting")).toBeInTheDocument();
			// Properties should be visible in the card
			expect(screen.getByText(/Date/i)).toBeInTheDocument();
			expect(screen.getByText(/2025-01-15/i)).toBeInTheDocument();
		});
	});

	it("should call onSelect when a note is clicked", async () => {
		const note: Note = {
			id: "click-note",
			content: "# Clickable Note",
			revision_id: "rev-click",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: NoteRecord = {
			id: "click-note",
			title: "Clickable Note",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};

		seedNote("ui-test-ws", note, record);

		const onSelect = vi.fn();
		render(() => <NoteList workspaceId="ui-test-ws" onSelect={onSelect} />);

		await waitFor(() => {
			expect(screen.getByText("Clickable Note")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByText("Clickable Note"));

		expect(onSelect).toHaveBeenCalledWith("click-note");
	});

	it("should show loading state", () => {
		render(() => <NoteList workspaceId="ui-test-ws" />);

		// Initially should show loading
		expect(screen.getByText(/loading/i)).toBeInTheDocument();
	});

	it("should highlight selected note", async () => {
		const note: Note = {
			id: "selected-note",
			content: "# Selected Note",
			revision_id: "rev-sel",
			created_at: "2025-01-01T00:00:00Z",
			updated_at: "2025-01-01T00:00:00Z",
		};
		const record: NoteRecord = {
			id: "selected-note",
			title: "Selected Note",
			updated_at: "2025-01-01T00:00:00Z",
			properties: {},
			tags: [],
			links: [],
		};

		seedNote("ui-test-ws", note, record);

		render(() => (
			<NoteList workspaceId="ui-test-ws" selectedNoteId="selected-note" />
		));

		await waitFor(() => {
			const noteItem = screen.getByText("Selected Note").closest("[data-testid='note-item']");
			expect(noteItem).toHaveClass("selected");
		});
	});
});
