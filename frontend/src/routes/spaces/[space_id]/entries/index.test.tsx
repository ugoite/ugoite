import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { http, HttpResponse } from "msw";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import SpaceEntriesIndexPane from "./index";
import { resetMockData, seedEntry, seedSpace } from "~/test/mocks/handlers";
import type { Entry, EntryRecord, Space } from "~/lib/types";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { createSpaceStore } from "~/lib/space-store";
import { createMemo, createSignal } from "solid-js";
import type { Form } from "~/lib/types";
import { server } from "~/test/mocks/server";
import { setLocale } from "~/lib/i18n";
import { testApiUrl } from "~/test/http-origin";

const navigateMock = vi.fn();
const searchParamsMock: Record<string, string> = {};
const setSearchParamsMock = vi.fn();
const createdAt = "2025-01-01T00:00:00Z";

const assetsForm: Form = {
	name: "Assets",
	version: 1,
	template: "",
	fields: {
		link: { type: "string", required: true },
		name: { type: "string", required: true },
		uploaded_at: { type: "timestamp", required: true },
	},
};

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useSearchParams: () => [searchParamsMock, setSearchParamsMock],
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

function renderEntriesRoute(options?: {
	spaceId?: string;
	forms?: Form[];
	loadingForms?: boolean;
	columnTypes?: string[];
	refetchForms?: () => void;
}) {
	const {
		spaceId = "default",
		forms = [],
		loadingForms = false,
		columnTypes = [],
		refetchForms = () => undefined,
	} = options ?? {};
	render(() => {
		const entryStore = createEntryStore(() => spaceId);
		const spaceStore = createSpaceStore();
		const [formsSignal] = createSignal<Form[]>(forms);
		const [loadingFormsSignal] = createSignal(loadingForms);
		const [columnTypesSignal] = createSignal<string[]>(columnTypes);
		return (
			<EntriesRouteContext.Provider
				value={{
					spaceStore,
					spaceId: () => spaceId,
					entryStore,
					forms: createMemo(() => formsSignal()),
					loadingForms: loadingFormsSignal,
					columnTypes: columnTypesSignal,
					refetchForms,
				}}
			>
				<SpaceEntriesIndexPane />
			</EntriesRouteContext.Provider>
		);
	});
}

function seedTestSpace(id: string, name = id) {
	const space: Space = {
		id,
		name,
		created_at: createdAt,
	};
	seedSpace(space);
}

describe("/spaces/:space_id/entries", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		setLocale("en");
		for (const key of Object.keys(searchParamsMock)) {
			delete searchParamsMock[key];
		}
		setSearchParamsMock.mockReset();
		resetMockData();
		seedTestSpace("default", "Default");

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
		renderEntriesRoute();

		await waitFor(() => {
			expect(screen.getByText("Test Entry")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByText("Test Entry"));
		expect(navigateMock).toHaveBeenCalledWith("/spaces/default/entries/entry%2Fwith%20space");
	});

	it("selecting an entry form navigates correctly", async () => {
		renderEntriesRoute();

		const formsTab = await screen.findByRole("link", { name: "Forms" });
		expect(formsTab).toHaveAttribute("href", "/spaces/default/forms");
	});

	it("REQ-FE-037: entries route disables entry creation when only reserved metadata forms exist", async () => {
		seedTestSpace("reserved-only", "Reserved Only");
		renderEntriesRoute({ spaceId: "reserved-only", forms: [assetsForm] });

		await waitFor(() => {
			expect(screen.getByText("Start by creating your first form.")).toBeInTheDocument();
		});
		expect(screen.getByRole("button", { name: "New entry" })).toBeDisabled();
		expect(screen.queryByText("No entries found.")).not.toBeInTheDocument();
	});

	it("REQ-FE-037: entries route guides first-run spaces toward form creation", async () => {
		seedTestSpace("empty-space", "Empty Space");
		renderEntriesRoute({ spaceId: "empty-space" });

		await waitFor(() => {
			expect(screen.getByText("Start by creating your first form.")).toBeInTheDocument();
		});
		expect(
			screen.getByText(
				"Entries depend on form templates and fields. Create one form first, then come back to add entries.",
			),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "New entry" })).toBeDisabled();

		fireEvent.click(screen.getByRole("button", { name: "Create your first form" }));

		expect(screen.getByRole("heading", { name: "Create New Form" })).toBeInTheDocument();
	});

	it("REQ-FE-037: entries route opens the create-entry dialog when creatable forms exist", async () => {
		seedTestSpace("creatable-space", "Creatable Space");
		renderEntriesRoute({
			spaceId: "creatable-space",
			forms: [
				{
					name: "Task",
					version: 1,
					template: "# Task\n\n## Summary\n",
					fields: {
						Summary: { type: "string", required: true },
					},
				},
			],
		});

		const newEntryButton = await screen.findByRole("button", { name: "New entry" });
		expect(newEntryButton).toBeEnabled();
		fireEvent.click(newEntryButton);

		expect(screen.getByRole("heading", { name: "Create New Entry" })).toBeInTheDocument();
	});

	it("REQ-FE-054: renders human-readable updated dates for query result cards", async () => {
		searchParamsMock.session = "session-1";
		server.use(
			http.get(testApiUrl("/spaces/default/sql-sessions/session-1"), () =>
				HttpResponse.json({
					id: "session-1",
					space_id: "default",
					sql_id: "query-1",
					sql: "SELECT 1",
					status: "ready",
					created_at: "2026-03-01T00:00:00Z",
					expires_at: "2026-03-01T01:00:00Z",
				}),
			),
			http.get(testApiUrl("/spaces/default/sql-sessions/session-1/rows"), () =>
				HttpResponse.json({
					rows: [
						{
							id: "query-entry",
							title: "Query Entry",
							form: "Meeting",
							updated_at: 1772960822.056,
							properties: {},
							tags: [],
							links: [],
						},
					],
					offset: 0,
					limit: 24,
					total_count: 1,
				}),
			),
		);

		renderEntriesRoute();

		const expectedDate = new Date(1772960822.056 * 1000).toLocaleDateString();
		expect(await screen.findByText("Query Entry")).toBeInTheDocument();
		expect(await screen.findByText(`Updated ${expectedDate}`)).toBeInTheDocument();
		expect(screen.queryByText("Updated 1772960822.056")).not.toBeInTheDocument();
	});

	it("REQ-FE-044: localizes entries route CTA copy in Japanese", async () => {
		setLocale("ja");
		seedTestSpace("empty-ja", "empty-ja");
		renderEntriesRoute({ spaceId: "empty-ja" });

		expect(await screen.findByRole("heading", { name: "エントリ" })).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "新しいエントリ" })).toBeInTheDocument();
		expect(await screen.findByText("最初のフォームを作成して始めましょう。")).toBeInTheDocument();
		expect(
			screen.getByText(
				"エントリはフォームのテンプレートとフィールドをもとに作成します。先に1つフォームを作成してからエントリ作成に戻ってください。",
			),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "最初のフォームを作成" })).toBeInTheDocument();
	});
});
