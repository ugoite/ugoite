import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import NotFoundRoute from "./[...404]";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("404 route", () => {
	it("REQ-E2E-004: unknown routes keep users inside Ugoite recovery paths", () => {
		render(() => <NotFoundRoute />);

		expect(screen.getByRole("heading", { name: "Page not found" })).toBeInTheDocument();
		expect(screen.getByText(/still inside Ugoite/i)).toBeInTheDocument();
		expect(screen.queryByText(/Visit solidjs.com/i)).not.toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Open Spaces" })).toHaveAttribute("href", "/spaces");
		expect(screen.getByRole("link", { name: "Go to Login" })).toHaveAttribute("href", "/login");
		expect(screen.getByRole("link", { name: "Back to Home" })).toHaveAttribute("href", "/");
		expect(screen.getByRole("link", { name: "About Ugoite" })).toHaveAttribute("href", "/about");
	});
});
