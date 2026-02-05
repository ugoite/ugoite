// REQ-FE-038: Form validation feedback in editor
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { EntryDetailPane } from "./EntryDetailPane";
import { entryApi } from "~/lib/entry-api";
import { assetApi } from "~/lib/asset-api";

vi.mock("~/lib/entry-api", () => {
	class RevisionConflictError extends Error {}
	return {
		entryApi: {
			get: vi.fn(),
			update: vi.fn(),
			delete: vi.fn(),
		},
		RevisionConflictError,
	};
});

vi.mock("~/lib/asset-api", () => ({
	assetApi: {
		list: vi.fn(),
		upload: vi.fn(),
	},
}));

describe("EntryDetailPane", () => {
	beforeEach(() => {
		vi.resetAllMocks();
		(assetApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
	});

	it("REQ-FE-038: renders form validation warnings", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: "Meeting",
			content: "---\nform: Meeting\n---\n# Test Entry\n\n## Date\n",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		(entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
			new Error(
				'Form validation failed: [{"field":"Date","message":"Missing required field: Date"}]',
			),
		);

		render(() => (
			<EntryDetailPane spaceId={() => "default"} entryId={() => "entry-1"} onDeleted={vi.fn()} />
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		fireEvent.input(textarea, { target: { value: "Updated content" } });

		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(screen.getByText("Form validation failed")).toBeInTheDocument();
			expect(screen.getByText("Missing required field: Date")).toBeInTheDocument();
		});
	});
});
