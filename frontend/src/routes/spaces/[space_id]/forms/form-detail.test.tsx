import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import { createMemo, createSignal } from "solid-js";
import SpaceFormDetailRoute from "./[form_name]";
import { resetMockData, seedForm, seedSpace } from "~/test/mocks/handlers";
import type { Form, Space } from "~/lib/types";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";

const navigateMock = vi.fn();
const paramsMock: { form_name?: string; space_id?: string } = {};

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useParams: () => paramsMock,
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

vi.mock("~/lib/search-api", () => ({
	searchApi: {
		query: vi.fn().mockResolvedValue([
			{
				id: "entry/with space",
				title: "Test Entry",
				form: "Project",
				updated_at: "2026-01-01T00:00:00Z",
				properties: { status: "Open" },
				links: [],
			},
		]),
	},
}));

describe("/spaces/:space_id/forms/:form_name", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		paramsMock.form_name = "Project";
		paramsMock.space_id = "default";
		resetMockData();

		const ws: Space = {
			id: "default",
			name: "Default",
			created_at: "2025-01-01T00:00:00Z",
		};
		seedSpace(ws);

		const cls: Form = {
			name: "Project",
			fields: { status: { type: "text" } },
		};
		seedForm("default", cls);
	});

	it("REQ-FE-033: form detail navigation encodes entry id", async () => {
		render(() => {
			const entryStore = createEntryStore(() => "default");
			const spaceStore = createSpaceStore();
			const [forms] = createSignal<Form[]>([
				{
					name: "Project",
					fields: { status: { type: "text" } },
				},
			]);
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
					<SpaceFormDetailRoute />
				</EntriesRouteContext.Provider>
			);
		});

		await waitFor(() => {
			expect(screen.getByText("Test Entry")).toBeInTheDocument();
		});

		const viewButtons = screen.getAllByLabelText("View Entry");
		fireEvent.click(viewButtons[0]);

		expect(navigateMock).toHaveBeenCalledWith("/spaces/default/entries/entry%2Fwith%20space");
	});
});
