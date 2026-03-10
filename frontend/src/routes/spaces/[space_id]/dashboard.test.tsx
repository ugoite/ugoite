import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@solidjs/testing-library";
import SpaceDashboardRoute from "./dashboard";
import { spaceApi } from "~/lib/space-api";
import { formApi } from "~/lib/form-api";
import { assetApi } from "~/lib/asset-api";
import type { Form } from "~/lib/types";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useParams: () => ({ space_id: "default" }),
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

vi.mock("~/components/SpaceShell", () => ({
	SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

vi.mock("~/components/AssetUploader", () => ({
	AssetUploader: () => <div>Asset uploader</div>,
}));

vi.mock("~/lib/entry-store", () => ({
	createEntryStore: () => ({
		createEntry: vi.fn(),
	}),
}));

vi.mock("~/lib/space-api", () => ({
	spaceApi: {
		get: vi.fn(),
	},
}));

vi.mock("~/lib/form-api", () => ({
	formApi: {
		list: vi.fn(),
		listTypes: vi.fn(),
		create: vi.fn(),
	},
}));

vi.mock("~/lib/asset-api", () => ({
	assetApi: {
		list: vi.fn(),
		upload: vi.fn(),
		delete: vi.fn(),
	},
}));

describe("/spaces/:space_id/dashboard", () => {
	const meetingForm: Form = {
		name: "Meeting",
		version: 1,
		template: "",
		fields: { Date: { type: "date", required: true } },
	};
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

	beforeEach(() => {
		navigateMock.mockReset();
		(spaceApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "default",
			name: "Default Space",
			created_at: "2025-01-01T00:00:00Z",
		});
		(formApi.listTypes as ReturnType<typeof vi.fn>).mockResolvedValue([]);
		(assetApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
	});

	it("REQ-FE-037: dashboard ignores reserved metadata forms in the entry count", async () => {
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([assetsForm, meetingForm]);

		render(() => <SpaceDashboardRoute />);

		await waitFor(() => {
			expect(screen.getByText("1 forms available")).toBeInTheDocument();
		});
		expect(screen.queryByText("2 forms available")).not.toBeInTheDocument();
	});

	it("REQ-FE-037: dashboard disables entry creation when only reserved metadata forms exist", async () => {
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([assetsForm]);

		render(() => <SpaceDashboardRoute />);

		await waitFor(() => {
			expect(screen.getByText("Create a form first to start writing entries.")).toBeInTheDocument();
		});
		expect(screen.getByRole("button", { name: "New entry" })).toBeDisabled();
	});

	it("REQ-FE-058: dashboard avoids a persistent top-level loading banner during routine navigation", () => {
		(spaceApi.get as ReturnType<typeof vi.fn>).mockImplementation(
			() => new Promise(() => undefined) as Promise<never>,
		);
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([meetingForm]);

		render(() => <SpaceDashboardRoute />);

		expect(screen.getByRole("heading", { name: "default" })).toBeInTheDocument();
		expect(screen.queryByText("Loading space...")).not.toBeInTheDocument();
	});

	it("REQ-FE-058: dashboard replaces the fallback title when space metadata loads", async () => {
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([meetingForm]);

		render(() => <SpaceDashboardRoute />);

		await waitFor(() => {
			expect(screen.getByRole("heading", { name: "Default Space" })).toBeInTheDocument();
		});
		expect(screen.queryByRole("heading", { name: "default" })).not.toBeInTheDocument();
	});
});
