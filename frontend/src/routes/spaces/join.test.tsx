import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SpaceInvitationJoinRoute from "./join";
import { spaceApi } from "~/lib/space-api";

// REQ-SEC-007: browser invitation acceptance flow
const navigateMock = vi.fn();

vi.mock("@solidjs/router", () => ({
	useNavigate: () => navigateMock,
	useSearchParams: () => [{ space_id: "default", token: "prefilled-token" }],
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

vi.mock("~/lib/space-api", () => ({
	spaceApi: {
		acceptInvitation: vi.fn(),
	},
}));

describe("/spaces/join", () => {
	beforeEach(() => {
		navigateMock.mockReset();
		(spaceApi.acceptInvitation as ReturnType<typeof vi.fn>).mockReset();
	});

	it("prefills invitation parameters from the query string", () => {
		render(() => <SpaceInvitationJoinRoute />);

		expect(screen.getByDisplayValue("default")).toBeInTheDocument();
		expect(screen.getByDisplayValue("prefilled-token")).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Back to Spaces" })).toHaveAttribute("href", "/spaces");
	});

	it("REQ-SEC-007: accepts an invitation token and confirms the joined membership", async () => {
		(spaceApi.acceptInvitation as ReturnType<typeof vi.fn>).mockResolvedValue({
			member: {
				user_id: "joined-user",
				role: "editor",
				state: "active",
			},
		});

		render(() => <SpaceInvitationJoinRoute />);

		fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

		await waitFor(() => {
			expect(spaceApi.acceptInvitation).toHaveBeenCalledWith("default", {
				token: "prefilled-token",
			});
		});
		expect(screen.getByText(/Invitation accepted\./i)).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Open space dashboard" })).toHaveAttribute(
			"href",
			"/spaces/default/dashboard",
		);
	});

	it("shows invitation errors inline", async () => {
		(spaceApi.acceptInvitation as ReturnType<typeof vi.fn>).mockRejectedValue(
			new Error("Invalid invitation token"),
		);

		render(() => <SpaceInvitationJoinRoute />);

		fireEvent.click(screen.getByRole("button", { name: "Accept invitation" }));

		await waitFor(() => {
			expect(screen.getByText("Invalid invitation token")).toBeInTheDocument();
		});
	});
});
