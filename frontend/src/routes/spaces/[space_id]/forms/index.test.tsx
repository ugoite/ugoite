import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { createMemo, createSignal } from "solid-js";
import SpaceFormsIndexPane from "./index";
import { EntriesRouteContext } from "~/lib/entries-route-context";
import { createEntryStore } from "~/lib/entry-store";
import { setLocale } from "~/lib/i18n";
import { createSpaceStore } from "~/lib/space-store";
import type { Form } from "~/lib/types";

const navigateMock = vi.fn();
const searchParamsMock: Record<string, string> = {};
const setSearchParamsMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useSearchParams: () => [searchParamsMock, setSearchParamsMock],
}));

vi.mock("~/components/SpaceShell", () => ({
	SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

vi.mock("~/components/FormTable", () => ({
	FormTable: (props: { entryForm: { name: string } }) => (
		<div>Form table: {props.entryForm.name}</div>
	),
}));

vi.mock("~/components/create-dialogs", () => ({
	CreateFormDialog: () => <div>Create form dialog</div>,
}));

vi.mock("~/lib/sql-session-api", () => ({
	sqlSessionApi: {
		get: vi.fn(),
		rows: vi.fn(),
	},
}));

describe("/spaces/:space_id/forms", () => {
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
	const meetingForm: Form = {
		name: "Meeting",
		version: 1,
		template: "",
		fields: { Date: { type: "date", required: true } },
	};

	beforeEach(() => {
		navigateMock.mockReset();
		setLocale("en");
		setSearchParamsMock.mockReset();
		for (const key of Object.keys(searchParamsMock)) {
			delete searchParamsMock[key];
		}
	});

	function renderPane(forms: Form[]) {
		render(() => {
			const entryStore = createEntryStore(() => "default");
			const spaceStore = createSpaceStore();
			const [formList] = createSignal(forms);
			const [loadingForms] = createSignal(false);
			const [columnTypes] = createSignal<string[]>([]);
			return (
				<EntriesRouteContext.Provider
					value={{
						spaceStore,
						spaceId: () => "default",
						entryStore,
						forms: createMemo(() => formList()),
						loadingForms,
						columnTypes,
						refetchForms: () => undefined,
					}}
				>
					<SpaceFormsIndexPane />
				</EntriesRouteContext.Provider>
			);
		});
	}

	it("REQ-FE-037: form grid defaults to the first creatable form", async () => {
		renderPane([assetsForm, meetingForm]);

		await waitFor(() => {
			expect(setSearchParamsMock).toHaveBeenCalledWith({ form: "Meeting" }, { replace: true });
		});
		expect(screen.queryByRole("option", { name: "Assets" })).not.toBeInTheDocument();
		expect(screen.getByRole("option", { name: "Meeting" })).toBeInTheDocument();
	});

	it("REQ-FE-037: form grid shows empty state when only reserved metadata forms exist", async () => {
		renderPane([assetsForm]);

		await waitFor(() => {
			expect(screen.getByText("Create a form to get started.")).toBeInTheDocument();
		});
		expect(setSearchParamsMock).not.toHaveBeenCalled();
		expect(screen.queryByRole("option", { name: "Assets" })).not.toBeInTheDocument();
	});

	it("REQ-FE-044: localizes forms route CTA copy in Japanese", async () => {
		setLocale("ja");
		renderPane([meetingForm]);

		await waitFor(() => {
			expect(screen.getByRole("heading", { name: "フォームグリッド" })).toBeInTheDocument();
		});
		expect(screen.getByRole("button", { name: "新しいフォーム" })).toBeInTheDocument();
		expect(screen.getByRole("option", { name: "Meeting" })).toBeInTheDocument();
	});
});
