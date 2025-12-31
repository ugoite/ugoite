import "@testing-library/jest-dom/vitest";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { NoteList } from "./NoteList";
import { resetMockData, seedWorkspace, seedNote } from "~/test/mocks/handlers";
import type { Note, NoteRecord, Workspace } from "~/lib/types";

const testWorkspace: Workspace = {
	id: "ui-test-ws",
	name: "UI Test Workspace",
	created_at: "2025-01-01T00:00:00Z",
};

// Helper to create controlled props
const createControlledProps = (initialNotes: NoteRecord[] = []) => {
	const [notes, setNotes] = createSignal(initialNotes);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);
	return { notes, setNotes, loading, setLoading, error, setError };
};

describe("NoteList", () => {
	beforeEach(() => {
		resetMockData();
		seedWorkspace(testWorkspace);
	});

	describe("controlled mode", () => {
		it("should render empty state when no notes exist", async () => {
			const { notes, loading, error } = createControlledProps();
			render(() => <NoteList notes={notes} loading={loading} error={error} />);

			expect(screen.getByText(/no notes/i)).toBeInTheDocument();
		});

		it("should render list of notes with titles", async () => {
			const record1: NoteRecord = {
				id: "note-1",
				title: "First Note",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};
			const record2: NoteRecord = {
				id: "note-2",
				title: "Second Note",
				updated_at: "2025-01-02T00:00:00Z",
				properties: { Status: "Active" },
				tags: [],
				links: [],
			};

			const { notes, loading, error } = createControlledProps([record1, record2]);
			render(() => <NoteList notes={notes} loading={loading} error={error} />);

			expect(screen.getByText("First Note")).toBeInTheDocument();
			expect(screen.getByText("Second Note")).toBeInTheDocument();
		});

		it("should display extracted properties in note cards", async () => {
			const record: NoteRecord = {
				id: "prop-note",
				title: "Meeting",
				updated_at: "2025-01-01T00:00:00Z",
				properties: { Date: "2025-01-15", Status: "Completed" },
				tags: [],
				links: [],
			};

			const { notes, loading, error } = createControlledProps([record]);
			render(() => <NoteList notes={notes} loading={loading} error={error} />);

			expect(screen.getByText("Meeting")).toBeInTheDocument();
			expect(screen.getByText(/Date/i)).toBeInTheDocument();
			expect(screen.getByText(/2025-01-15/i)).toBeInTheDocument();
		});

		it("should call onSelect when a note is clicked", async () => {
			const record: NoteRecord = {
				id: "click-note",
				title: "Clickable Note",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};

			const { notes, loading, error } = createControlledProps([record]);
			const onSelect = vi.fn();
			render(() => <NoteList notes={notes} loading={loading} error={error} onSelect={onSelect} />);

			fireEvent.click(screen.getByText("Clickable Note"));

			expect(onSelect).toHaveBeenCalledWith("click-note");
		});

		it("should show loading state", () => {
			const { notes, loading, setLoading, error } = createControlledProps();
			setLoading(true);
			render(() => <NoteList notes={notes} loading={loading} error={error} />);

			expect(screen.getByText(/loading/i)).toBeInTheDocument();
		});

		it("should highlight selected note", async () => {
			const record: NoteRecord = {
				id: "selected-note",
				title: "Selected Note",
				updated_at: "2025-01-01T00:00:00Z",
				properties: {},
				tags: [],
				links: [],
			};

			const { notes, loading, error } = createControlledProps([record]);
			render(() => (
				<NoteList notes={notes} loading={loading} error={error} selectedNoteId="selected-note" />
			));

			const button = screen.getByRole("button");
			expect(button).toHaveClass("selected");
		});
	});

	describe("standalone mode", () => {
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
	});
});
