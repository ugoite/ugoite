import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import NotesRoute from "../notes";
import { resetMockData, seedNote, seedWorkspace } from "~/test/mocks/handlers";
import type { Note, NoteRecord, Workspace } from "~/lib/types";

const navigateMock = vi.fn();
const paramsMock: { noteId?: string } = {};

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useParams: () => paramsMock,
}));

describe("/notes (layout route)", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		paramsMock.noteId = undefined;
		resetMockData();
		const ws: Workspace = {
			id: "default",
			name: "Default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(ws);

		const note: Note = {
			id: "note-1",
			content: "# Test Note\n\nBody",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		};
		const record: NoteRecord = {
			id: "note-1",
			title: "Test Note",
			class: null,
			updated_at: "2026-01-01T00:00:00Z",
			created_at: "2026-01-01T00:00:00Z",
			properties: {},
			links: [],
		};
		seedNote("default", note, record);
	});

	it("REQ-FE-008: selecting a note navigates to /notes/:id", async () => {
		render(() => (
			<NotesRoute>
				<div data-testid="route-children" />
			</NotesRoute>
		));

		await waitFor(() => {
			expect(screen.getByText("Test Note")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByText("Test Note"));
		expect(navigateMock).toHaveBeenCalledWith("/notes/note-1");
	});
});
