// REQ-FE-017: Workspace storage configuration
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { WorkspaceSettings } from "./WorkspaceSettings";
import type { Workspace } from "~/lib/types";

const mockWorkspace: Workspace = {
	id: "ws-1",
	name: "Test Workspace",
	storage_config: {
		uri: "file:///local/path",
	},
};

describe("WorkspaceSettings", () => {
	it("should display workspace name", () => {
		render(() => <WorkspaceSettings workspace={mockWorkspace} onSave={vi.fn()} />);
		expect(screen.getByDisplayValue("Test Workspace")).toBeInTheDocument();
	});

	it("should display storage config", () => {
		render(() => <WorkspaceSettings workspace={mockWorkspace} onSave={vi.fn()} />);
		expect(screen.getByDisplayValue("file:///local/path")).toBeInTheDocument();
	});

	it("should call onSave when save button is clicked", async () => {
		const onSave = vi.fn().mockResolvedValue({});
		render(() => <WorkspaceSettings workspace={mockWorkspace} onSave={onSave} />);

		const nameInput = screen.getByLabelText(/workspace name/i);
		fireEvent.input(nameInput, { target: { value: "Updated Workspace" } });

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(onSave).toHaveBeenCalledWith({
				name: "Updated Workspace",
				storage_config: { uri: "file:///local/path" },
			});
		});
	});

	it("should test connection when test button is clicked", async () => {
		const onTestConnection = vi.fn().mockResolvedValue({ status: "ok" });
		render(() => (
			<WorkspaceSettings
				workspace={mockWorkspace}
				onSave={vi.fn()}
				onTestConnection={onTestConnection}
			/>
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
			<WorkspaceSettings
				workspace={mockWorkspace}
				onSave={vi.fn()}
				onTestConnection={onTestConnection}
			/>
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
			<WorkspaceSettings
				workspace={mockWorkspace}
				onSave={vi.fn()}
				onTestConnection={onTestConnection}
			/>
		));

		const testButton = screen.getByRole("button", { name: /test connection/i });
		fireEvent.click(testButton);

		await waitFor(() => {
			expect(screen.getByText(/connection failed/i)).toBeInTheDocument();
		});
	});

	it("should save storage config", async () => {
		const onSave = vi.fn().mockResolvedValue({});
		render(() => <WorkspaceSettings workspace={mockWorkspace} onSave={onSave} />);

		const uriInput = screen.getByLabelText(/storage uri/i);
		fireEvent.input(uriInput, { target: { value: "s3://my-bucket/ieapp" } });

		const saveButton = screen.getByRole("button", { name: /save/i });
		fireEvent.click(saveButton);

		await waitFor(() => {
			expect(onSave).toHaveBeenCalledWith({
				name: "Test Workspace",
				storage_config: { uri: "s3://my-bucket/ieapp" },
			});
		});
	});
});
