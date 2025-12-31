import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { WorkspaceSelector } from "./WorkspaceSelector";
import type { Workspace } from "~/lib/types";

describe("WorkspaceSelector", () => {
	const mockWorkspaces: Workspace[] = [
		{ id: "ws-1", name: "Workspace One", created_at: "2025-01-01T00:00:00Z" },
		{ id: "ws-2", name: "Workspace Two", created_at: "2025-01-01T00:00:00Z" },
	];

	it("should render workspace options", () => {
		const onSelect = vi.fn();
		const onCreate = vi.fn();

		render(() => (
			<WorkspaceSelector
				workspaces={mockWorkspaces}
				selectedWorkspaceId="ws-1"
				loading={false}
				error={null}
				onSelect={onSelect}
				onCreate={onCreate}
			/>
		));

		const select = screen.getByRole("combobox");
		expect(select).toBeInTheDocument();

		const options = screen.getAllByRole("option");
		expect(options).toHaveLength(2);
		expect(options[0]).toHaveTextContent("Workspace One");
		expect(options[1]).toHaveTextContent("Workspace Two");
	});

	it("should show loading state", () => {
		const onSelect = vi.fn();
		const onCreate = vi.fn();

		render(() => (
			<WorkspaceSelector
				workspaces={[]}
				selectedWorkspaceId={null}
				loading={true}
				error={null}
				onSelect={onSelect}
				onCreate={onCreate}
			/>
		));

		expect(screen.getByText("Loading...")).toBeInTheDocument();
	});

	it("should show error message", () => {
		const onSelect = vi.fn();
		const onCreate = vi.fn();

		render(() => (
			<WorkspaceSelector
				workspaces={mockWorkspaces}
				selectedWorkspaceId="ws-1"
				loading={false}
				error="Failed to load"
				onSelect={onSelect}
				onCreate={onCreate}
			/>
		));

		expect(screen.getByText("Failed to load")).toBeInTheDocument();
	});

	it("should call onSelect when workspace changes", async () => {
		const onSelect = vi.fn();
		const onCreate = vi.fn();

		render(() => (
			<WorkspaceSelector
				workspaces={mockWorkspaces}
				selectedWorkspaceId="ws-1"
				loading={false}
				error={null}
				onSelect={onSelect}
				onCreate={onCreate}
			/>
		));

		const select = screen.getByRole("combobox");
		await fireEvent.change(select, { target: { value: "ws-2" } });

		expect(onSelect).toHaveBeenCalledWith("ws-2");
	});

	it("should show create form when add button clicked", async () => {
		const onSelect = vi.fn();
		const onCreate = vi.fn();

		render(() => (
			<WorkspaceSelector
				workspaces={mockWorkspaces}
				selectedWorkspaceId="ws-1"
				loading={false}
				error={null}
				onSelect={onSelect}
				onCreate={onCreate}
			/>
		));

		const addButton = screen.getByTitle("Create new workspace");
		await fireEvent.click(addButton);

		expect(screen.getByPlaceholderText("New workspace name...")).toBeInTheDocument();
		expect(screen.getByText("Create")).toBeInTheDocument();
		expect(screen.getByText("Cancel")).toBeInTheDocument();
	});

	it("should call onCreate when form submitted", async () => {
		const onSelect = vi.fn();
		const onCreate = vi.fn();

		render(() => (
			<WorkspaceSelector
				workspaces={mockWorkspaces}
				selectedWorkspaceId="ws-1"
				loading={false}
				error={null}
				onSelect={onSelect}
				onCreate={onCreate}
			/>
		));

		// Open create form
		const addButton = screen.getByTitle("Create new workspace");
		await fireEvent.click(addButton);

		// Enter name and submit
		const input = screen.getByPlaceholderText("New workspace name...");
		await fireEvent.input(input, { target: { value: "New Workspace" } });

		const createButton = screen.getByText("Create");
		await fireEvent.click(createButton);

		expect(onCreate).toHaveBeenCalledWith("New Workspace");
	});

	it("should hide create form when cancel clicked", async () => {
		const onSelect = vi.fn();
		const onCreate = vi.fn();

		render(() => (
			<WorkspaceSelector
				workspaces={mockWorkspaces}
				selectedWorkspaceId="ws-1"
				loading={false}
				error={null}
				onSelect={onSelect}
				onCreate={onCreate}
			/>
		));

		// Open create form
		const addButton = screen.getByTitle("Create new workspace");
		await fireEvent.click(addButton);

		expect(screen.getByPlaceholderText("New workspace name...")).toBeInTheDocument();

		// Cancel
		const cancelButton = screen.getByText("Cancel");
		await fireEvent.click(cancelButton);

		expect(screen.queryByPlaceholderText("New workspace name...")).not.toBeInTheDocument();
	});
});
