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

	it("REQ-FE-038: shows markdown H2 guidance and inserts missing required sections", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: "Meeting",
			content: "---\nform: Meeting\n---\n\n# Test Entry\n\n## Notes\nhello",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});

		render(() => (
			<EntryDetailPane
				spaceId={() => "default"}
				entryId={() => "entry-1"}
				forms={() => [
					{
						name: "Meeting",
						version: 1,
						template: "# Meeting\n\n## Date\n\n## Notes\n",
						fields: {
							Date: { type: "string", required: true },
							Notes: { type: "markdown", required: false },
						},
					},
				]}
				onDeleted={vi.fn()}
			/>
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());
		expect(await screen.findByText(/必須セクション不足: Date/)).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "不足H2を追加" }));

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		expect((textarea as HTMLTextAreaElement).value).toContain("## Date");
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

	it("shows error message when entry load fails", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("Network error"));

		render(() => (
			<EntryDetailPane
				spaceId={() => "default"}
				entryId={() => "missing-entry"}
				onDeleted={vi.fn()}
			/>
		));

		await waitFor(() => {
			expect(screen.getByText("Network error")).toBeInTheDocument();
		});
	});

	it("calls assetApi.upload when file is uploaded", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: null,
			content: "# Test Entry",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		const mockAsset = { id: "asset-1", name: "file.txt", path: "/path/file.txt" };
		(assetApi.upload as ReturnType<typeof vi.fn>).mockResolvedValue(mockAsset);
		(assetApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([mockAsset]);

		render(() => (
			<EntryDetailPane spaceId={() => "default"} entryId={() => "entry-1"} onDeleted={vi.fn()} />
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

		// Wait for the file input to be present (after entry loads)
		const fileInput = await waitFor(() => {
			const el = document.querySelector('input[type="file"]') as HTMLInputElement;
			if (!el) throw new Error("file input not found");
			return el;
		});

		const file = new File(["content"], "file.txt", { type: "text/plain" });
		fireEvent.change(fileInput, { target: { files: [file] } });

		await waitFor(() => {
			expect(assetApi.upload).toHaveBeenCalledWith("default", file);
		});
	});

	it("saves successfully and marks editor clean", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: null,
			content: "# Test Entry",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		(entryApi.update as ReturnType<typeof vi.fn>).mockResolvedValue({
			revision_id: "rev-2",
		});
		const onAfterSave = vi.fn();

		render(() => (
			<EntryDetailPane
				spaceId={() => "default"}
				entryId={() => "entry-1"}
				onDeleted={vi.fn()}
				onAfterSave={onAfterSave}
			/>
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		fireEvent.input(textarea, { target: { value: "Updated content" } });
		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(entryApi.update).toHaveBeenCalled();
			expect(onAfterSave).toHaveBeenCalled();
		});
	});

	it("shows unknown fields warning from save error", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: null,
			content: "# Test Entry",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		(entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
			new Error("Unknown form fields: extraField1, extraField2"),
		);

		render(() => (
			<EntryDetailPane spaceId={() => "default"} entryId={() => "entry-1"} onDeleted={vi.fn()} />
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		fireEvent.input(textarea, { target: { value: "Updated content" } });
		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(screen.getByText("Unknown form fields")).toBeInTheDocument();
		});
	});

	it("handles malformed JSON in validation error message", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: null,
			content: "# Test Entry",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		(entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
			new Error("Form validation failed: not-valid-json"),
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
		});
	});

	it("shows conflict message on generic save error", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: null,
			content: "# Test Entry",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		(entryApi.update as ReturnType<typeof vi.fn>).mockRejectedValue(
			new Error("Server unavailable"),
		);

		render(() => (
			<EntryDetailPane spaceId={() => "default"} entryId={() => "entry-1"} onDeleted={vi.fn()} />
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		fireEvent.input(textarea, { target: { value: "Updated content" } });
		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(screen.getByText("Server unavailable")).toBeInTheDocument();
		});
	});

	it("calls onDeleted after successful delete", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-1",
			title: "Test Entry",
			form: null,
			content: "# Test Entry",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});
		(entryApi.delete as ReturnType<typeof vi.fn>).mockResolvedValue(undefined);
		const onDeleted = vi.fn();
		vi.stubGlobal("confirm", () => true);

		render(() => (
			<EntryDetailPane spaceId={() => "default"} entryId={() => "entry-1"} onDeleted={onDeleted} />
		));

		// Wait for entry header to appear (entry is loaded)
		await waitFor(() => screen.getByRole("button", { name: "Delete" }));

		fireEvent.click(screen.getByRole("button", { name: "Delete" }));

		await waitFor(() => {
			expect(entryApi.delete).toHaveBeenCalledWith("default", "entry-1");
			expect(onDeleted).toHaveBeenCalled();
		});

		vi.unstubAllGlobals();
	});
});
