import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import SpaceEntriesIndexPane from "./index";
import { resetMockData, seedEntry, seedSpace } from "~/test/mocks/handlers";
import type { Entry, EntryRecord, Space } from "~/lib/types";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";
import { createMemo, createSignal } from "solid-js";
import type { Form } from "~/lib/types";

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

describe("/spaces/:space_id/entries", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		for (const key of Object.keys(searchParamsMock)) {
			delete searchParamsMock[key];
		}
		setSearchParamsMock.mockReset();
		resetMockData();
		const ws: Space = {
			id: "default",
			name: "Default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedSpace(ws);

		const entry: Entry = {
			id: "entry/with space",
			content: "# Test Entry\n\nBody",
			revision_id: "rev-1",
			created_at: "2026-01-01T00:00:00Z",
			updated_at: "2026-01-01T00:00:00Z",
		};
		const record: EntryRecord = {
			id: "entry/with space",
			title: "Test Entry",
			updated_at: "2026-01-01T00:00:00Z",
			properties: {},
			links: [],
		};
		seedEntry("default", entry, record);
	});

	it("REQ-FE-033: selecting an entry navigates with encoded id", async () => {
		render(() => {
			const entryStore = createEntryStore(() => "default");
			const spaceStore = createSpaceStore();
			const [forms] = createSignal<Form[]>([]);
			const [loadingForms] = createSignal(false);
			const [columnTypes] = createSignal<string[]>([]);
			return (
				<EntriesRouteContext.Provider
					value={{
						spaceStore,
						spaceId: () => "default",
						entryStore,
						forms: createMemo(() => forms()),
						loadingForms,
						columnTypes,
						refetchForms: () => undefined,
					}}
				>
					<SpaceEntriesIndexPane />
				</EntriesRouteContext.Provider>
			);
		});

		await waitFor(() => {
			expect(screen.getByText("Test Entry")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByText("Test Entry"));
		expect(navigateMock).toHaveBeenCalledWith("/spaces/default/entries/entry%2Fwith%20space");
	});

	it("selecting an entry form navigates correctly", async () => {
		render(() => {
			const entryStore = createEntryStore(() => "default");
			const spaceStore = createSpaceStore();
			const [forms] = createSignal<Form[]>([]);
			const [loadingForms] = createSignal(false);
			const [columnTypes] = createSignal<string[]>([]);
			return (
				<EntriesRouteContext.Provider
					value={{
						spaceStore,
						spaceId: () => "default",
						entryStore,
						forms: createMemo(() => forms()),
						loadingForms,
						columnTypes,
						refetchForms: () => undefined,
					}}
				>
					<SpaceEntriesIndexPane />
				</EntriesRouteContext.Provider>
			);
		});

		const gridTab = await screen.findByRole("link", { name: "grid" });
		expect(gridTab).toHaveAttribute("href", "/spaces/default/forms");
	});
});
