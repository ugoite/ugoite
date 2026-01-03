import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { AttachmentUploader } from "./AttachmentUploader";
import type { Attachment } from "~/lib/types";

describe("AttachmentUploader", () => {
	it("should render file input", () => {
		render(() => <AttachmentUploader onUpload={vi.fn()} />);
		const input = screen.getByLabelText(/upload attachment/i);
		expect(input).toBeInTheDocument();
		expect(input).toHaveAttribute("type", "file");
	});

	it("should call onUpload when file is selected", async () => {
		const onUpload = vi.fn<[File], Promise<Attachment>>().mockResolvedValue({
			id: "att-123",
			name: "test.pdf",
			path: "attachments/att-123_test.pdf",
		});

		render(() => <AttachmentUploader onUpload={onUpload} />);

		const input = screen.getByLabelText(/upload attachment/i) as HTMLInputElement;
		const file = new File(["test content"], "test.pdf", { type: "application/pdf" });

		Object.defineProperty(input, "files", {
			value: [file],
			writable: false,
		});

		fireEvent.change(input);

		await waitFor(() => {
			expect(onUpload).toHaveBeenCalledWith(file);
		});
	});

	it("should display uploading state", async () => {
		const onUpload = vi.fn().mockImplementation(() => new Promise(() => {})); // Never resolves

		render(() => <AttachmentUploader onUpload={onUpload} />);

		const input = screen.getByLabelText(/upload attachment/i) as HTMLInputElement;
		const file = new File(["test"], "test.txt", { type: "text/plain" });

		Object.defineProperty(input, "files", {
			value: [file],
			writable: false,
		});

		fireEvent.change(input);

		await waitFor(() => {
			expect(screen.getByText(/uploading/i)).toBeInTheDocument();
		});
	});

	it("should display uploaded attachments", () => {
		const attachments: Attachment[] = [
			{ id: "att-1", name: "doc.pdf", path: "attachments/att-1_doc.pdf" },
			{ id: "att-2", name: "image.png", path: "attachments/att-2_image.png" },
		];

		render(() => <AttachmentUploader onUpload={vi.fn()} attachments={attachments} />);

		expect(screen.getByText("doc.pdf")).toBeInTheDocument();
		expect(screen.getByText("image.png")).toBeInTheDocument();
	});

	it("should allow removing attachments", () => {
		const attachments: Attachment[] = [
			{ id: "att-1", name: "doc.pdf", path: "attachments/att-1_doc.pdf" },
		];
		const onRemove = vi.fn();

		render(() => (
			<AttachmentUploader onUpload={vi.fn()} attachments={attachments} onRemove={onRemove} />
		));

		const removeButton = screen.getByLabelText(/remove.*doc\.pdf/i);
		fireEvent.click(removeButton);

		expect(onRemove).toHaveBeenCalledWith("att-1");
	});

	it("should accept specific file types", () => {
		render(() => <AttachmentUploader onUpload={vi.fn()} accept=".pdf,.doc" />);

		const input = screen.getByLabelText(/upload attachment/i);
		expect(input).toHaveAttribute("accept", ".pdf,.doc");
	});

	it("should display error message on upload failure", async () => {
		const onUpload = vi.fn().mockRejectedValue(new Error("Upload failed"));

		render(() => <AttachmentUploader onUpload={onUpload} />);

		const input = screen.getByLabelText(/upload attachment/i) as HTMLInputElement;
		const file = new File(["test"], "test.txt", { type: "text/plain" });

		Object.defineProperty(input, "files", {
			value: [file],
			writable: false,
		});

		fireEvent.change(input);

		await waitFor(() => {
			expect(screen.getByText(/upload failed/i)).toBeInTheDocument();
		});
	});
});
