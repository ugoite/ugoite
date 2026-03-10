import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { render, screen } from "@solidjs/testing-library";
import SpaceSearchRoute from "./search";
import { resetMockData, seedSpace } from "~/test/mocks/handlers";
import { server } from "~/test/mocks/server";
import type { Space } from "~/lib/types";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
	useNavigate: () => navigateMock,
	useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/components/SpaceShell", () => ({
	SpaceShell: (props: { children: unknown }) => <div>{props.children}</div>,
}));

describe("/spaces/:space_id/search", () => {
	const testSpace: Space = {
		id: "default",
		name: "Default",
		created_at: "2025-01-01T00:00:00Z",
	};

	beforeEach(() => {
		navigateMock.mockReset();
		resetMockData();
		seedSpace(testSpace);
	});

	it("REQ-FE-054: renders human-readable updated dates in query search results", async () => {
		server.use(
			http.get("http://localhost:3000/api/spaces/default/sql", () =>
				HttpResponse.json([
					{
						id: "query-1",
						name: "Recent Query",
						sql: "SELECT 1",
						variables: [],
						created_at: 1772960822.056,
						updated_at: 1772960822.056,
						revision_id: "rev-1",
					},
				]),
			),
		);

		render(() => <SpaceSearchRoute />);

		const expectedDate = new Date(1772960822.056 * 1000).toLocaleDateString();
		expect(await screen.findByText("Recent Query")).toBeInTheDocument();
		expect(await screen.findByText(`Updated ${expectedDate}`)).toBeInTheDocument();
		expect(screen.queryByText("Updated 1772960822.056")).not.toBeInTheDocument();
	});
});
