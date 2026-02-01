// REQ-FE-038: Class validation feedback in editor
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { NoteDetailPane } from "./NoteDetailPane";
import { noteApi } from "~/lib/note-api";
import { attachmentApi } from "~/lib/attachment-api";

vi.mock("~/lib/note-api", () => {
	class RevisionConflictError extends Error {}
	return {
		noteApi: {
			get: vi.fn(),
			update: vi.fn(),
			delete: vi.fn(),
		},
		RevisionConflictError,
	};
});

vi.mock("~/lib/attachment-api", () => ({
	attachmentApi: {
		list: vi.fn(),
		upload: vi.fn(),
	},
}));

describe("NoteDetailPane", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		(attachmentApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
	});

	it("REQ-FE-038: renders class validation warnings", async () => {
		(noteApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "note-1",
			title: "Test Note",
			class: "Meeting",
			content: "---\nclass: Meeting\n---\n# Test Note\n\n## Date\n",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		(noteApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
			new Error(
				'Class validation failed: [{"field":"Date","message":"Missing required field: Date"}]',
			),
		);

		render(() => (
			<NoteDetailPane workspaceId={() => "default"} noteId={() => "note-1"} onDeleted={vi.fn()} />
		));

		await waitFor(() => expect(noteApi.get).toHaveBeenCalled());

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		fireEvent.input(textarea, { target: { value: "Updated content" } });

		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(screen.getByText("Class validation failed")).toBeInTheDocument();
			expect(screen.getByText("Missing required field: Date")).toBeInTheDocument();
		});
	});
});
