// REQ-FE-017: Space storage configuration
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { SpaceSettings } from "./SpaceSettings";
import type { Space } from "~/lib/types";

const mockSpace: Space = {
	id: "ws-1",
	name: "Test Space",
	created_at: "2025-01-01T00:00:00Z",
	storage: {
		type: "local",
		root: "/var/lib/ugoite/current",
	},
	storage_config: {
		uri: "s3://planned-bucket/test-space",
	},
};

describe("SpaceSettings", () => {
	it("should display space name", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(screen.getByDisplayValue("Test Space")).toBeInTheDocument();
	});

	it("should display storage config", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(screen.getByDisplayValue("s3://planned-bucket/test-space")).toBeInTheDocument();
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
				storage_config: { uri: "s3://planned-bucket/test-space" },
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
			expect(onTestConnection).toHaveBeenCalledWith({ uri: "s3://planned-bucket/test-space" });
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
		fireEvent.input(uriInput, { target: { value: "file:///var/lib/ugoite/migrated" } });

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(onSave).toHaveBeenCalledWith({
				name: "Test Space",
				storage_config: { uri: "file:///var/lib/ugoite/migrated" },
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

	it("REQ-FE-017: explains that saved storage URIs are metadata-only before migration", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(screen.getByText(/saved uri below is migration metadata/i)).toBeInTheDocument();
		expect(
			screen.getByText(/backend still writes through the deployment-wide storage root/i),
		).toBeInTheDocument();
		expect(
			screen.getByText(/does not migrate existing entries or assets to the new location/i),
		).toBeInTheDocument();
		expect(
			screen.getByText(
				/does not reroute writes until per-space routing or migration support lands/i,
			),
		).toBeInTheDocument();
	});

	it("REQ-FE-017: frames connector URIs as future migration targets", () => {
		render(() => <SpaceSettings space={mockSpace} onSave={vi.fn()} />);
		expect(
			screen.getByText(/records a local path you may want to migrate this space to later/i),
		).toBeInTheDocument();
		expect(
			screen.getByText(/local paths keep control and offline access on this machine/i),
		).toBeInTheDocument();
		expect(
			screen.getByText(
				/records an object-storage target you may want to validate or migrate to later/i,
			),
		).toBeInTheDocument();
		expect(
			screen.getByText(/team access and backups, but it adds cloud credentials and usage costs/i),
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
		expect(screen.getByText("file:///var/lib/ugoite/current")).toBeInTheDocument();
		expect(screen.getByDisplayValue("s3://planned-bucket/test-space")).toBeInTheDocument();
	});
});
