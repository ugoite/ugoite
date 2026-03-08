import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import SpacesIndexRoute from "./index";
import { spaceApi } from "~/lib/space-api";

const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

vi.mock("~/lib/space-api", () => ({
	spaceApi: {
		list: vi.fn(),
		create: vi.fn(),
	},
}));

describe("/spaces", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		(spaceApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
		(spaceApi.create as ReturnType<typeof vi.fn>).mockReset();
	});

	it("REQ-FE-002: shows a create-space action when no spaces exist", async () => {
		render(() => <SpacesIndexRoute />);

		await waitFor(() => {
			expect(screen.getByText("No spaces available.")).toBeInTheDocument();
		});

		expect(screen.getByRole("button", { name: "Create space" })).toBeInTheDocument();
		expect(spaceApi.create).not.toHaveBeenCalled();
	});

	it("REQ-FE-002: creates a space only after explicit user submission", async () => {
		(spaceApi.create as ReturnType<typeof vi.fn>).mockResolvedValue({
			id: "my-space",
			name: "my-space",
		});

		render(() => <SpacesIndexRoute />);

		await waitFor(() => {
			expect(screen.getByText("No spaces available.")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByRole("button", { name: "Create space" }));
		fireEvent.input(screen.getByLabelText("Space name"), {
			target: { value: "my-space" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Create space" }));

		await waitFor(() => {
			expect(spaceApi.create).toHaveBeenCalledWith("my-space");
			expect(navigateMock).toHaveBeenCalledWith("/spaces/my-space/dashboard");
		});
	});
});
