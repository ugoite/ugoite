import "@testing-library/jest-dom/vitest";
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import HomeRoute from "./index";

vi.mock("@solidjs/router", () => ({
	A: (props: { href: string; class?: string; children: unknown }) => (
		<a href={props.href} class={props.class}>
			{props.children}
		</a>
	),
}));

describe("home route", () => {
	it("REQ-E2E-008: public home page routes Learn More to the canonical getting-started docsite flow", () => {
		render(() => <HomeRoute />);

		expect(screen.getByRole("heading", { name: "Ugoite" })).toBeInTheDocument();
		expect(screen.getByRole("link", { name: "Learn More" })).toHaveAttribute(
			"href",
			"https://ugoite.github.io/ugoite/getting-started",
		);
	});
});
