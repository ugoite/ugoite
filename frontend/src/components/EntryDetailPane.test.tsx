// REQ-FE-038: Form validation feedback in editor
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { EntryDetailPane } from "./EntryDetailPane";
import { entryApi, RevisionConflictError } from "~/lib/entry-api";
import { assetApi } from "~/lib/asset-api";
import { setLocale } from "~/lib/i18n";
import type { Form } from "~/lib/types";

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
		setLocale("en");
		(assetApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
	});

	it("REQ-FE-052: shows form-aware markdown H2 guidance and inserts missing required sections", async () => {
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
		const guidanceText = await screen.findByText(
			(_, element) =>
				element?.tagName === "P" && Boolean(element.textContent?.match(/Form:\s*Meeting\s*\/\s*Example:/)),
		);
		expect(guidanceText).toHaveTextContent(/Form:\s*Meeting/);
		expect(guidanceText).toHaveTextContent(/Example:\s*##\s*Date/);
		expect(screen.queryByText("## status")).not.toBeInTheDocument();
		expect(await screen.findByText(/Missing required sections: Date/)).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "Insert missing H2 headings" }));

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		expect((textarea as HTMLTextAreaElement).value).toContain("## Date");
	});

	it("REQ-FE-052: omits example heading when form has no fields", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-2",
			title: "Scratch Note",
			form: "Empty",
			content: "---\nform: Empty\n---\n\n# Scratch Note",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});

		render(() => (
			<EntryDetailPane
				spaceId={() => "default"}
				entryId={() => "entry-2"}
				forms={() => [
					{
						name: "Empty",
						version: 1,
						template: "# Empty\n",
						fields: {},
					},
				]}
				onDeleted={vi.fn()}
			/>
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());
		await waitFor(() => expect(screen.getAllByText("Scratch Note")).toHaveLength(2));
		const guidanceText = screen.getByText(
			(_, element) =>
				element?.tagName === "P" && Boolean(element.textContent?.match(/Form:\s*Empty/)),
		);
		expect(guidanceText).not.toHaveTextContent(/Example:/);
		expect(screen.queryByText("## status")).not.toBeInTheDocument();
	});

	it("REQ-FE-052: omits example heading when form data lacks a fields map", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-3",
			title: "Broken Note",
			form: "Broken",
			content: "---\nform: Broken\n---\n\n# Broken Note",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});

		render(() => (
			<EntryDetailPane
				spaceId={() => "default"}
				entryId={() => "entry-3"}
				forms={() => [
					{
						name: "Broken",
						version: 1,
						template: "# Broken\n",
					} as unknown as Form,
				]}
				onDeleted={vi.fn()}
			/>
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());
		await waitFor(() => expect(screen.getAllByText("Broken Note")).toHaveLength(2));
		const guidanceText = screen.getByText(
			(_, element) =>
				element?.tagName === "P" && Boolean(element.textContent?.match(/Form:\s*Broken/)),
		);
		expect(guidanceText).not.toHaveTextContent(/Example:/);
		expect(screen.queryByText("## status")).not.toBeInTheDocument();
	});

	it("REQ-FE-053: renders English editor guidance and type warnings", async () => {
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-4",
			title: "Task Entry",
			form: "Task",
			content:
				"---\nform: Task\n---\n\n# Task Entry\n\n## Summary\nhello\n\n## Done\nmaybe\n\n## Extra\nvalue",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});

		render(() => (
			<EntryDetailPane
				spaceId={() => "default"}
				entryId={() => "entry-4"}
				forms={() => [
					{
						name: "Task",
						version: 1,
						template: "# Task\n\n## Summary\n\n## Done\n",
						fields: {
							Summary: { type: "string", required: true },
							Done: { type: "boolean", required: false },
						},
					},
				]}
				onDeleted={vi.fn()}
			/>
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());
		await waitFor(() => expect(screen.getAllByText("Task Entry")).toHaveLength(2));
		expect(await screen.findByText(/Enter attributes under/i)).toBeInTheDocument();
		const guidanceText = screen.getByText(
			(_, element) =>
				element?.tagName === "P" &&
				(element.textContent?.includes("Enter attributes under ## field name headings.") ?? false),
		);
		expect(guidanceText).toBeInTheDocument();
		const formGuidanceText = screen.getByText(
			(_, element) =>
				element?.tagName === "P" && Boolean(element.textContent?.match(/Form:\s*Task\s*\/\s*Example:/)),
		);
		expect(formGuidanceText).toHaveTextContent(/Form:\s*Task\s*\/\s*Example:\s*##\s*Summary/);
		expect(screen.getByText(/Unknown sections: Extra/)).toBeInTheDocument();
		expect(
			screen.getByText("Done: Use true/false, yes/no, on/off, or 1/0 for boolean fields."),
		).toBeInTheDocument();
	});

	it("REQ-FE-053: renders Japanese editor guidance without mixed-language examples", async () => {
		setLocale("ja");
		(entryApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "entry-ja",
			title: "タスク",
			form: "Task",
			content: "---\nform: Task\n---\n\n# タスク\n\n## Summary\nhello",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		});

		render(() => (
			<EntryDetailPane
				spaceId={() => "default"}
				entryId={() => "entry-ja"}
				forms={() => [
					{
						name: "Task",
						version: 1,
						template: "# Task\n\n## Summary\n",
						fields: {
							Summary: { type: "string", required: true },
						},
					},
				]}
				onDeleted={vi.fn()}
			/>
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());
		await waitFor(() => expect(screen.getAllByText("タスク")).toHaveLength(2));
		expect(
			screen.getByText(
				(_, element) =>
					element?.tagName === "P" &&
					(element.textContent?.includes("属性は ## フィールド名 見出しで入力します。") ?? false),
			),
		).toBeInTheDocument();
		const guidanceText = screen.getByText(
			(_, element) =>
				element?.tagName === "P" && Boolean(element.textContent?.includes("フォーム: Task")),
		);
		expect(guidanceText).toHaveTextContent(/フォーム:\s*Task\s*\/\s*例:\s*##\s*Summary/);
		expect(guidanceText).not.toHaveTextContent(/Example:/);
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

	it("REQ-FE-009: shows refresh guidance on revision conflict", async () => {
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
			new RevisionConflictError("Revision conflict", "server-rev"),
		);

		render(() => (
			<EntryDetailPane spaceId={() => "default"} entryId={() => "entry-1"} onDeleted={vi.fn()} />
		));

		await waitFor(() => expect(entryApi.get).toHaveBeenCalled());

		const textarea = await screen.findByPlaceholderText("Start writing in Markdown...");
		fireEvent.input(textarea, { target: { value: "Updated content" } });
		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => {
			expect(
				screen.getByText(
					"This entry was modified elsewhere. Your draft is still in the editor; refresh to load the latest version.",
				),
			).toBeInTheDocument();
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
