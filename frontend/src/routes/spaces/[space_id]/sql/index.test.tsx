import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import SpaceSqlRoute from "./index";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
	useParams: () => ({ space_id: "default" }),
}));

vi.mock("~/components/SpaceShell", () => ({
	SpaceShell: (props: { children: unknown; spaceId: string; activeTopTab?: string }) => (
		<div data-space-id={props.spaceId} data-active-top-tab={props.activeTopTab}>
			{props.children}
		</div>
	),
}));

describe("/spaces/:space_id/sql", () => {
	it("REQ-FE-061: saved SQL route explains the missing UI and links to working recovery paths", () => {
		const { container } = render(() => <SpaceSqlRoute />);

		expect(screen.getByRole("heading", { name: "Saved SQL" })).toBeInTheDocument();
		expect(screen.getByText(/named-query management/i)).toBeInTheDocument();
		expect(
			screen.queryByText("Saved SQL management is not yet in the UI."),
		).not.toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Open Search" })).toHaveAttribute(
			"href",
			"/spaces/default/search",
		);
		expect(screen.getByRole("link", { name: "Back to Dashboard" })).toHaveAttribute(
			"href",
			"/spaces/default/dashboard",
		);
		expect(screen.getByRole("link", { name: "Browse Entries" })).toHaveAttribute(
			"href",
			"/spaces/default/entries",
		);
		expect(container.firstElementChild).toHaveAttribute("data-space-id", "default");
		expect(container.firstElementChild).toHaveAttribute("data-active-top-tab", "search");
	});
});
