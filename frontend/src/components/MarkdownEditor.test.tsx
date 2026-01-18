// REQ-FE-005: Markdown editor
// REQ-FE-005a: Editor content graceful handling
// REQ-FE-009: Conflict message display
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { MarkdownEditor } from "./MarkdownEditor";

describe("MarkdownEditor", () => {
	it("should render textarea with initial content", () => {
		render(() => <MarkdownEditor content="# Hello World" onChange={() => {}} />);

		const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
		expect(textarea.value).toBe("# Hello World");
	});

	it("should call onChange when content is edited", async () => {
		const onChange = vi.fn();
		render(() => <MarkdownEditor content="# Initial" onChange={onChange} />);

		const textarea = screen.getByRole("textbox");
		fireEvent.input(textarea, { target: { value: "# Updated" } });

		expect(onChange).toHaveBeenCalledWith("# Updated");
	});

	it("should render markdown preview when preview mode is enabled", async () => {
		render(() => <MarkdownEditor content="# Preview Test" onChange={() => {}} showPreview />);

		// Should show preview toggle
		const previewButton = screen.getByRole("button", { name: /preview/i });
		expect(previewButton).toBeInTheDocument();
	});

	it("should be readonly when disabled", () => {
		render(() => <MarkdownEditor content="# Readonly" onChange={() => {}} disabled />);

		const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
		expect(textarea.disabled).toBe(true);
	});

	it("should show save indicator when dirty", () => {
		render(() => <MarkdownEditor content="# Test" onChange={() => {}} isDirty />);

		expect(screen.getByText(/unsaved/i)).toBeInTheDocument();
	});

	it("should call onSave when save button is clicked", async () => {
		const onSave = vi.fn();
		render(() => <MarkdownEditor content="# Test" onChange={() => {}} isDirty onSave={onSave} />);

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		expect(onSave).toHaveBeenCalled();
	});

	it("should support keyboard shortcut for save", async () => {
		const onSave = vi.fn();
		render(() => <MarkdownEditor content="# Test" onChange={() => {}} isDirty onSave={onSave} />);

		const textarea = screen.getByRole("textbox");
		fireEvent.keyDown(textarea, { key: "s", metaKey: true });

		expect(onSave).toHaveBeenCalled();
	});

	it("should show conflict message when there is a conflict", () => {
		render(() => (
			<MarkdownEditor
				content="# Test"
				onChange={() => {}}
				conflictMessage="Revision mismatch - please refresh"
			/>
		));

		expect(screen.getByText(/revision mismatch/i)).toBeInTheDocument();
	});

	it("should handle undefined content gracefully", () => {
		// @ts-expect-error Testing undefined content handling
		render(() => <MarkdownEditor content={undefined} onChange={() => {}} />);

		const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
		// Should not show "undefined" as text, should be empty
		expect(textarea.value).toBe("");
		expect(textarea.value).not.toBe("undefined");
	});

	it("should handle null content gracefully", () => {
		// @ts-expect-error Testing null content handling
		render(() => <MarkdownEditor content={null} onChange={() => {}} />);

		const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
		// Should not show "null" as text, should be empty
		expect(textarea.value).toBe("");
		expect(textarea.value).not.toBe("null");
	});

	it("should render preview with undefined content without crashing", async () => {
		// @ts-expect-error Testing undefined content handling in preview
		render(() => <MarkdownEditor content={undefined} onChange={() => {}} showPreview />);

		const previewButton = screen.getByRole("button", { name: /preview/i });
		fireEvent.click(previewButton);

		// Should toggle to preview mode without crashing
		expect(screen.getByRole("button", { name: /edit/i })).toBeInTheDocument();
	});

	it("should display placeholder when content is empty", () => {
		render(() => <MarkdownEditor content="" onChange={() => {}} placeholder="Start typing..." />);

		const textarea = screen.getByPlaceholderText("Start typing...");
		expect(textarea).toBeInTheDocument();
	});
});
