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
		fireEvent.input(uriInput, { target: { value: "s3://my-bucket/ugoite" } });

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(onSave).toHaveBeenCalledWith({
				name: "Test Space",
				storage_config: { uri: "s3://my-bucket/ugoite" },
			});
		});
	});

	it("should show error when save fails", async () => {
		const onSave = vi.fn().mockRejectedValue(new Error("Save failed"));
		render(() => <SpaceSettings space={mockSpace} onSave={onSave} />);

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(screen.getByText("Save failed")).toBeInTheDocument();
		});
	});

	it("renders with space that has no storage_config", () => {
		const space = { id: "test", name: "Test", created_at: "2025-01-01T00:00:00Z" };
		render(() => <SpaceSettings space={space as any} onSave={vi.fn()} />);
		const uriInput = screen.getByPlaceholderText(/file:\/\/\/local\/path/i);
		expect(uriInput).toHaveValue("");
	});

	it("test connection button not shown when onTestConnection not provided", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(screen.queryByRole("button", { name: /test connection/i })).not.toBeInTheDocument();
	});

	it("REQ-FE-017: explains storage trade-offs before changing the URI", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(
			screen.getByText(/local paths keep control and offline access on this machine/i),
		).toBeInTheDocument();
		expect(
			screen.getByText(/team access and backups but adds cloud credentials and usage costs/i),
		).toBeInTheDocument();
	});

	it("REQ-FE-017: warns that changing the URI does not migrate existing data automatically", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(
			screen.getByText(/does not migrate existing entries or assets to the new location/i),
		).toBeInTheDocument();
		expect(screen.getByRole("link", { name: /storage migration guide/i })).toHaveAttribute(
			"href",
			expect.stringContaining("/docs/guide/storage-migration"),
		);
	});

	it("REQ-FE-060: settings show the current storage topology before editing", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(screen.getByText("Storage topology")).toBeInTheDocument();
		expect(screen.getByText("Local filesystem")).toBeInTheDocument();
		expect(screen.getByText("file:///local/path")).toBeInTheDocument();
	});
});
