import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import WorkspaceNotesIndexPane from "./index";
import { resetMockData, seedNote, seedWorkspace } from "~/test/mocks/handlers";
import type { Note, NoteRecord, Workspace } from "~/lib/types";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { createNoteStore } from "~/lib/note-store";
import { createWorkspaceStore } from "~/lib/workspace-store";
import { createMemo, createSignal } from "solid-js";
import type { Class } from "~/lib/types";

const navigateMock = vi.fn();
const searchParamsMock: Record<string, string> = {};
const setSearchParamsMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useSearchParams: () => [searchParamsMock, setSearchParamsMock],
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("/workspaces/:workspace_id/notes", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		for (const key of Object.keys(searchParamsMock)) {
			delete searchParamsMock[key];
		}
		setSearchParamsMock.mockReset();
		resetMockData();
		const ws: Workspace = {
			id: "default",
			name: "Default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedWorkspace(ws);

		const note: Note = {
			id: "note/with space",
			content: "# Test Note\n\nBody",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		};
		const record: NoteRecord = {
			id: "note/with space",
			title: "Test Note",
			updated_at: "2026-01-01T00:00:00Z",
			properties: {},
			links: [],
		};
		seedNote("default", note, record);
	});

	it("REQ-FE-033: selecting a note navigates with encoded id", async () => {
		render(() => {
			const noteStore = createNoteStore(() => "default");
			const workspaceStore = createWorkspaceStore();
			const [classes] = createSignal<Class[]>([]);
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
					<WorkspaceNotesIndexPane />
				</NotesRouteContext.Provider>
			);
		});

		await waitFor(() => {
			expect(screen.getByText("Test Note")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByText("Test Note"));
		expect(navigateMock).toHaveBeenCalledWith("/workspaces/default/notes/note%2Fwith%20space");
	});

	it("selecting a note class navigates correctly", async () => {
		render(() => {
			const noteStore = createNoteStore(() => "default");
			const workspaceStore = createWorkspaceStore();
			const [classes] = createSignal<Class[]>([]);
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
					<WorkspaceNotesIndexPane />
				</NotesRouteContext.Provider>
			);
		});

		const gridTab = await screen.findByRole("link", { name: "grid" });
		expect(gridTab).toHaveAttribute("href", "/workspaces/default/classes");
	});
});
