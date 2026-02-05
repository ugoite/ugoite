// REQ-FE-015: Asset upload UI
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { AssetUploader } from "./AssetUploader";
import type { Asset } from "~/lib/types";

describe("AssetUploader", () => {
	it("should render file input", () => {
		render(() => <AssetUploader onUpload={vi.fn()} />);
		const input = screen.getByLabelText(/upload asset/i);
		expect(input).toBeInTheDocument();
		expect(input).toHaveAttribute("type", "file");
	});

	it("should call onUpload when file is selected", async () => {
		const onUpload = vi.fn<[File], Promise<Asset>>().mockResolvedValue({
			id: "att-123",
			name: "test.pdf",
			path: "assets/att-123_test.pdf",
		});

		render(() => <AssetUploader onUpload={onUpload} />);

		const input = screen.getByLabelText(/upload asset/i) as HTMLInputElement;
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

		render(() => <AssetUploader onUpload={onUpload} />);

		const input = screen.getByLabelText(/upload asset/i) as HTMLInputElement;
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

	it("should display uploaded assets", () => {
		const assets: Asset[] = [
			{ id: "att-1", name: "doc.pdf", path: "assets/att-1_doc.pdf" },
			{ id: "att-2", name: "image.png", path: "assets/att-2_image.png" },
		];

		render(() => <AssetUploader onUpload={vi.fn()} assets={assets} />);

		expect(screen.getByText("doc.pdf")).toBeInTheDocument();
		expect(screen.getByText("image.png")).toBeInTheDocument();
	});

	it("should allow removing assets", () => {
		const assets: Asset[] = [{ id: "att-1", name: "doc.pdf", path: "assets/att-1_doc.pdf" }];
		const onRemove = vi.fn();

		render(() => <AssetUploader onUpload={vi.fn()} assets={assets} onRemove={onRemove} />);

		const removeButton = screen.getByLabelText(/remove.*doc\.pdf/i);
		fireEvent.click(removeButton);

		expect(onRemove).toHaveBeenCalledWith("att-1");
	});

	it("should accept specific file types", () => {
		render(() => <AssetUploader onUpload={vi.fn()} accept=".pdf,.doc" />);

		const input = screen.getByLabelText(/upload asset/i);
		expect(input).toHaveAttribute("accept", ".pdf,.doc");
	});

	it("should display error message on upload failure", async () => {
		const onUpload = vi.fn().mockRejectedValue(new Error("Upload failed"));

		render(() => <AssetUploader onUpload={onUpload} />);

		const input = screen.getByLabelText(/upload asset/i) as HTMLInputElement;
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
