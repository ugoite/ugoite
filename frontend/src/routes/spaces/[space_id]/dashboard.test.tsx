import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import SpaceDashboardRoute from "./dashboard";
import { spaceApi } from "~/lib/space-api";
import { formApi } from "~/lib/form-api";
import { assetApi } from "~/lib/asset-api";
import { setLocale } from "~/lib/i18n";
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
		setLocale("en");
		(spaceApi.get as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "default",
			name: "Default Space",
			created_at: "2025-01-01T00:00:00Z",
			storage: { type: "local", root: "/var/lib/ugoite/default" },
			storage_config: { uri: "s3://planned-bucket/default" },
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
			expect(screen.getByText("Start by creating your first form.")).toBeInTheDocument();
		});
		expect(screen.getByRole("button", { name: "New entry" })).toBeDisabled();
	});

	it("REQ-FE-037: dashboard promotes form creation when a new space has no creatable forms", async () => {
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);

		render(() => <SpaceDashboardRoute />);

		await waitFor(() => {
			expect(screen.getByText("Start by creating your first form.")).toBeInTheDocument();
		});
		expect(
			screen.getByText(
				"Entries depend on form templates and fields. Create one form first, then come back to add entries.",
			),
		).toBeInTheDocument();
		expect(screen.getByText("Recommended first step")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "New entry" })).toBeDisabled();

		fireEvent.click(screen.getByRole("button", { name: "Create your first form" }));

		expect(screen.getByRole("heading", { name: "Create New Form" })).toBeInTheDocument();
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

	it("REQ-FE-044: dashboard localizes main workflow copy in Japanese", async () => {
		setLocale("ja");
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([meetingForm]);

		render(() => <SpaceDashboardRoute />);

		await waitFor(() => {
			expect(screen.getByRole("heading", { name: "エントリを作成" })).toBeInTheDocument();
		});
		expect(screen.getByRole("button", { name: "新しいエントリ" })).toBeInTheDocument();
		expect(screen.getByRole("heading", { name: "フォームを作成" })).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "新しいフォーム" })).toBeInTheDocument();
		expect(screen.getByRole("heading", { name: "アセット" })).toBeInTheDocument();
		expect(
			screen.getByText("ファイルをアップロードし、カタログのメタデータを同期します。"),
		).toBeInTheDocument();
	});

	it("REQ-FE-044: dashboard localizes first-run onboarding copy in Japanese", async () => {
		setLocale("ja");
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);

		render(() => <SpaceDashboardRoute />);

		await waitFor(() => {
			expect(screen.getByText("最初のフォームを作成して始めましょう。")).toBeInTheDocument();
		});
		expect(
			screen.getByText(
				"エントリはフォームのテンプレートとフィールドをもとに作成します。先に1つフォームを作成してからエントリ作成に戻ってください。",
			),
		).toBeInTheDocument();
		expect(screen.getByText("最初のおすすめステップ")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "最初のフォームを作成" })).toBeInTheDocument();
	});

	it("REQ-FE-060: dashboard surfaces the active storage topology with a settings link", async () => {
		(formApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([meetingForm]);

		render(() => <SpaceDashboardRoute />);

		await waitFor(() => {
			expect(screen.getByRole("heading", { name: "Storage topology" })).toBeInTheDocument();
		});
		expect(screen.getByText("Local filesystem")).toBeInTheDocument();
		expect(screen.getByText("file:///var/lib/ugoite/default")).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Open space settings" })).toHaveAttribute(
			"href",
			"/spaces/default/settings",
		);
	});
});
