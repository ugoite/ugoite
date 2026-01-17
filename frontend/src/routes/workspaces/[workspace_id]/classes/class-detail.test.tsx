import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { createMemo, createSignal } from "solid-js";
import WorkspaceClassDetailRoute from "./[class_name]";
import { resetMockData, seedClass, seedWorkspace } from "~/test/mocks/handlers";
import type { Class, Workspace } from "~/lib/types";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { createNoteStore } from "~/lib/note-store";
import { createWorkspaceStore } from "~/lib/workspace-store";

const navigateMock = vi.fn();
const paramsMock: { class_name?: string; workspace_id?: string } = {};

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useParams: () => paramsMock,
}));

vi.mock("~/lib/search-api", () => ({
	searchApi: {
		query: vi.fn().mockResolvedValue([
			{
				id: "note/with space",
				title: "Test Note",
				class: "Project",
				updated_at: "2026-01-01T00:00:00Z",
				properties: { status: "Open" },
				links: [],
			},
		]),
	},
}));

describe("/workspaces/:workspace_id/classes/:class_name", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		paramsMock.class_name = "Project";
		paramsMock.workspace_id = "default";
		resetMockData();

		const ws: Workspace = {
			id: "default",
			name: "Default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(ws);

		const cls: Class = {
			name: "Project",
			fields: { status: { type: "text" } },
		};
		seedClass("default", cls);
	});

	it("REQ-FE-033: class detail navigation encodes note id", async () => {
		render(() => {
			const noteStore = createNoteStore(() => "default");
			const workspaceStore = createWorkspaceStore();
			const [classes] = createSignal<Class[]>([
				{
					name: "Project",
					fields: { status: { type: "text" } },
				},
			]);
			const [loadingClasses] = createSignal(false);
			const [columnTypes] = createSignal<string[]>([]);

			return (
				<NotesRouteContext.Provider
					value={{
						workspaceStore,
						workspaceId: () => "default",
						noteStore,
						classes: createMemo(() => classes()),
						loadingClasses,
						columnTypes,
						refetchClasses: () => undefined,
					}}
				>
					<WorkspaceClassDetailRoute />
				</NotesRouteContext.Provider>
			);
		});

		await waitFor(() => {
			expect(screen.getByText("Test Note")).toBeInTheDocument();
		});

		const viewButtons = screen.getAllByLabelText("View Note");
		fireEvent.click(viewButtons[0]);

		expect(navigateMock).toHaveBeenCalledWith("/workspaces/default/notes/note%2Fwith%20space");
	});
});
