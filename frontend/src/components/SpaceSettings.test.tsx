// REQ-FE-017: Space storage configuration
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { SpaceSettings } from "./SpaceSettings";
import type { Space } from "~/lib/types";

const mockSpace: Space = {
	id: "ws-1",
	name: "Test Space",
	storage_config: {
		uri: "file:///local/path",
	},
};

describe("SpaceSettings", () => {
	it("should display space name", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(screen.getByDisplayValue("Test Space")).toBeInTheDocument();
	});

	it("should display storage config", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(screen.getByDisplayValue("file:///local/path")).toBeInTheDocument();
	});

	it("should call onSave when save button is clicked", async () => {
		const onSave = vi.fn().mockResolvedValue({});
		render(() => <SpaceSettings space={mockSpace} onSave={onSave} />);

		const nameInput = screen.getByLabelText(/space name/i);
		fireEvent.input(nameInput, { target: { value: "Updated Space" } });

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(onSave).toHaveBeenCalledWith({
				name: "Updated Space",
				storage_config: { uri: "file:///local/path" },
			});
		});
	});

	it("should test connection when test button is clicked", async () => {
		const onTestConnection = vi.fn().mockResolvedValue({ status: "ok" });
		render(() => (
			<SpaceSettings space={mockSpace} onSave={vi.fn()} onTestConnection={onTestConnection} />
		));

		const testButton = screen.getByRole("button", { name: /test connection/i });
		fireEvent.click(testButton);

		await waitFor(() => {
			expect(onTestConnection).toHaveBeenCalledWith({ uri: "file:///local/path" });
		});
	});

	it("should display success message after test connection", async () => {
		const onTestConnection = vi.fn().mockResolvedValue({ status: "ok" });
		render(() => (
			<SpaceSettings space={mockSpace} onSave={vi.fn()} onTestConnection={onTestConnection} />
		));

		const testButton = screen.getByRole("button", { name: /test connection/i });
		fireEvent.click(testButton);

		await waitFor(() => {
			expect(screen.getByText(/connection successful/i)).toBeInTheDocument();
		});
	});

	it("should display error message on test connection failure", async () => {
		const onTestConnection = vi.fn().mockRejectedValue(new Error("Connection failed"));
		render(() => (
			<SpaceSettings space={mockSpace} onSave={vi.fn()} onTestConnection={onTestConnection} />
		));

		const testButton = screen.getByRole("button", { name: /test connection/i });
		fireEvent.click(testButton);

		await waitFor(() => {
			expect(screen.getByText(/connection failed/i)).toBeInTheDocument();
		});
	});

	it("should save storage config", async () => {
		const onSave = vi.fn().mockResolvedValue({});
		render(() => <SpaceSettings space={mockSpace} onSave={onSave} />);

		const uriInput = screen.getByLabelText(/storage uri/i);
		fireEvent.input(uriInput, { target: { value: "s3://my-bucket/ieapp" } });

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(onSave).toHaveBeenCalledWith({
				name: "Test Space",
				storage_config: { uri: "s3://my-bucket/ieapp" },
			});
		});
	});
});
